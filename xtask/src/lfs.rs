use std::{io::Read, process::Stdio, str::FromStr};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use ureq::http::{HeaderMap, HeaderName, HeaderValue, request::Builder as RequestBuilder};

use crate::sha256::HexSha256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Upload,
    Download,
}

impl Operation {
    fn as_str(self) -> &'static str {
        match self {
            Operation::Upload => "upload",
            Operation::Download => "download",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Authorisation {
    operation: Operation,
    headers: HeaderMap,
}

fn remote_and_repo_from_ssh_url(url: &str) -> Result<(&str, &str)> {
    if !url.starts_with("ssh://") {
        bail!("Repo URL must be a valid SSH URL");
    }

    Ok(url[6..]
        .find(':')
        .map_or((url, ""), |i| (&url[..6 + i], &url[7 + i..])))
}

fn deserialize_header_map<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<HeaderMap, D::Error> {
    struct HeaderMapVisitor;

    impl<'de> serde::de::Visitor<'de> for HeaderMapVisitor {
        type Value = HeaderMap;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an http header map")
        }

        fn visit_map<A>(self, mut map: A) -> std::result::Result<Self::Value, A::Error>
        where
            A: serde::de::MapAccess<'de>,
        {
            let mut result = HeaderMap::new();

            while let Some((key, value)) = map.next_entry::<&str, &str>()? {
                let key = HeaderName::from_str(key).map_err(|_| {
                    serde::de::Error::invalid_value(
                        serde::de::Unexpected::Str(key),
                        &"value is not a valid http header name",
                    )
                })?;
                let value = HeaderValue::from_str(value).map_err(|_| {
                    serde::de::Error::invalid_value(
                        serde::de::Unexpected::Str(value),
                        &"value is not a valid http header value",
                    )
                })?;

                result.append(key, value);
            }

            Ok(result)
        }
    }

    deserializer.deserialize_map(HeaderMapVisitor)
}

impl Authorisation {
    fn from_json(json: &[u8], operation: Operation) -> Result<Self> {
        #[derive(Deserialize)]
        struct AuthorisationResponse {
            #[serde(rename = "header", deserialize_with = "deserialize_header_map")]
            headers: HeaderMap,
        }

        let AuthorisationResponse { headers } = serde_json::from_slice(json)?;
        Ok(Self { operation, headers })
    }

    pub fn authenticate_with_ssh(repo_url: &str, operation: Operation) -> Result<Self> {
        let (remote, repo_path) = remote_and_repo_from_ssh_url(repo_url)?;

        let repo_path_with_git = {
            if repo_path.ends_with(".git") {
                repo_path
            } else {
                &format!("{repo_path}.git")
            }
        };

        let output = std::process::Command::new("ssh")
            .arg("-T")
            .arg(remote)
            .arg("git-lfs-authenticate")
            .arg(repo_path_with_git)
            .arg(operation.as_str())
            .stderr(Stdio::inherit())
            .stdin(Stdio::inherit())
            .output()
            .context("Failed to authenticate with remote via ssh")?;

        if !output.status.success() {
            bail!("`ssh` failed with exit status: {}", output.status)
        }

        Self::from_json(&output.stdout, operation)
            .context("Failed to parse `git-lfs-authenticate` response")
    }
}

pub fn guess_api_url_from_repo_url(repo_url: &str) -> Result<String> {
    let (host, path) = if let Some(value) = repo_url.strip_prefix("ssh://") {
        value[value.find('@').map_or(0, |i| i + 1)..]
            .split_once(':')
            .context("SSH repo url missing `:`")?
    } else if let Some(value) = repo_url
        .strip_prefix("http://")
        .or_else(|| repo_url.strip_prefix("https://"))
    {
        value
            .split_once('/')
            .context("HTTP repo url missing a path")?
    } else {
        bail!("Unknown repo URL schema");
    };
    let stripped_path = path.strip_suffix(".git").unwrap_or(path);

    Ok(format!("https://{host}/{stripped_path}.git/info/lfs"))
}

pub struct Client {
    http_agent: ureq::Agent,
    api_base_url: String,
}

pub struct BatchObject {
    pub sha256: Box<HexSha256>,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BatchObjectHandle {
    #[serde(rename = "oid")]
    pub sha256: Box<HexSha256>,
    #[expect(dead_code)]
    pub size: u64,
    #[serde(default)]
    pub actions: BatchObjectActions,
}

#[derive(Default, Debug, Clone, Deserialize)]
pub struct BatchObjectActions {
    pub upload: Option<UploadAction>,
    pub verify: Option<VerifyAction>,
    pub download: Option<DownloadAction>,
}

#[derive(Debug, Clone, Deserialize)]
struct GenericAction {
    #[serde(rename = "href")]
    url: Box<str>,
    #[serde(
        default,
        rename = "header",
        deserialize_with = "deserialize_header_map"
    )]
    headers: HeaderMap,
}

macro_rules! make_action_wrapper {
    ($name: ident, $method: literal, ($($body_type: ty)?) -> $($response_type: tt)*) => {
        #[derive(Debug, Clone, Deserialize)]
        #[serde(transparent)]
        pub struct $name(GenericAction);

        impl $name {
            pub fn execute(&self, client: &Client $(, mut body: $body_type)?) -> Result<$($response_type)*> {
                let response_body = client.execute_generic_action(
                    $method,
                    &self.0,
                    make_action_wrapper!(@body_or_unit body $($body_type)?)
                )?;

                make_action_wrapper!(@extract_result response_body $($response_type)*)
            }
        }
    };
    (@body_or_unit $ident: ident $body_type: ty) => { ureq::AsSendBody::as_body(&mut $ident) };
    (@body_or_unit $ident: ident) => { ureq::AsSendBody::as_body(&mut ()) };
    (@extract_result $body: ident ()) => { Ok(_ = $body) };
    (@extract_result $body: ident impl Read) => {
        Ok($body.into_reader())
    };
}

make_action_wrapper!(UploadAction, "PUT", (impl ureq::AsSendBody) -> ());
make_action_wrapper!(VerifyAction, "POST", () -> ());
make_action_wrapper!(DownloadAction, "GET", () -> impl Read);

fn apply_header_map(mut builder: RequestBuilder, map: &HeaderMap) -> RequestBuilder {
    for (key, value) in map {
        builder = builder.header(key, value);
    }
    builder
}

impl Client {
    pub fn new(api_url: String) -> Self {
        Self {
            http_agent: ureq::Agent::new_with_config(
                ureq::config::Config::builder()
                    .https_only(true)
                    .user_agent("afishhh/subrandr xtask LFS client")
                    .build(),
            ),
            api_base_url: api_url,
        }
    }

    pub fn batch(
        &self,
        objects: impl IntoIterator<Item = BatchObject>,
        auth: Option<&Authorisation>,
        operation: Operation,
    ) -> Result<Vec<BatchObjectHandle>> {
        if let Some(auth) = auth {
            assert_eq!(auth.operation, operation);
        }

        let object_map: Vec<serde_json::Value> = objects
            .into_iter()
            .map(|BatchObject { sha256, size }| serde_json::json!({ "oid": sha256, "size": size }))
            .collect();
        let request = {
            let mut builder = RequestBuilder::new();
            if let Some(auth) = auth {
                builder = apply_header_map(builder, &auth.headers);
            }
            builder
                .method("POST")
                .uri(format!("{}/objects/batch", self.api_base_url))
                .header("Content-Type", "application/vnd.git-lfs+json")
                .header("Accept", "application/vnd.git-lfs+json")
                .body(
                    serde_json::json!({
                        "operation": operation.as_str(),
                        "transfers": ["basic"],
                        "objects": object_map,
                        "hash_algo": "sha256"
                    })
                    .to_string(),
                )?
        };

        #[derive(Deserialize)]
        struct Response {
            objects: Vec<BatchObjectHandle>,
        }

        let response = self.http_agent.run(request)?;
        Ok(serde_json::from_slice::<Response>(&response.into_body().read_to_vec()?)?.objects)
    }

    fn execute_generic_action(
        &self,
        method: &str,
        action: &GenericAction,
        body: ureq::SendBody,
    ) -> Result<ureq::Body> {
        let request = apply_header_map(RequestBuilder::new(), &action.headers)
            .method(method)
            .uri(&*action.url)
            .body(body)?;

        Ok(self.http_agent.run(request)?.into_body())
    }
}
