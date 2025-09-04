import copy
from dataclasses import dataclass
from datetime import datetime
import json
import subprocess as sp
import urllib.parse
import requests
from typing import Any, Literal, overload

_Operation = Literal["download"] | Literal["upload"]


@dataclass
class LfsAuthorization:
    headers: dict[str, str]
    expires_at: datetime


def _authenticate_ssh(
    remote_url: str, repo: str, operation: _Operation
) -> LfsAuthorization:
    parsed = urllib.parse.urlparse(remote_url)
    remote_url = urllib.parse.urlunparse(parsed)
    repo = repo + ".git"

    result = json.loads(
        sp.check_output(
            ["ssh", "-T", remote_url, "git-lfs-authenticate", repo, operation],
            encoding="utf8",
        )
    )

    return LfsAuthorization(
        headers=result["header"],
        expires_at=datetime.fromisoformat(result["expires_at"]),
    )


@dataclass(frozen=True)
class BatchObject:
    sha256: str
    size: int


@dataclass(frozen=True)
class Action:
    url: str
    headers: dict[str, str]
    expires_at: datetime | None


@dataclass(frozen=True)
class UploadAction(Action):
    pass


@dataclass(frozen=True)
class VerifyAction(Action):
    pass


@dataclass(frozen=True)
class DownloadAction(Action):
    pass


@dataclass(frozen=True)
class BatchHandle(BatchObject):
    upload: UploadAction | None
    verify: VerifyAction | None
    download: DownloadAction | None


class LfsClient:
    def __init__(
        self,
        remote_url: str,
        repo_name: str,
        api_url: str,
        authenticated_operation: _Operation | None = None,
    ) -> None:
        if authenticated_operation:
            self._authorization = _authenticate_ssh(
                remote_url, repo_name, authenticated_operation
            )
        else:
            self._authorization = None

        self._operation = authenticated_operation
        self._api_url = api_url
        self._session = requests.Session()

    def _request(self, method: str, path: str, body: Any) -> requests.PreparedRequest:
        assert path.startswith("/")

        headers = {}
        if self._authorization:
            headers = copy.copy(self._authorization.headers)
        headers["Accept"] = "application/vnd.git-lfs+json"
        headers["Content-Type"] = "application/vnd.git-lfs+json"

        return requests.Request(
            method, f"{self._api_url}{path}", headers=headers, json=body
        ).prepare()

    def batch(
        self, objects: list[BatchObject], operation: _Operation
    ) -> list[BatchHandle]:
        if len(objects) == 0:
            return []

        response = self._session.send(
            self._request(
                "POST",
                "/objects/batch",
                {
                    "operation": operation,
                    "transfers": ["basic"],
                    "objects": [
                        {"oid": object.sha256, "size": object.size}
                        for object in objects
                    ],
                    "hash_algo": "sha256",
                },
            )
        )

        response.raise_for_status()
        result = response.json()

        handles: list[BatchHandle] = []
        for object in result["objects"]:
            actions = object.get("actions", {})

            def create_action(ctor, value: dict[str, Any] | None):
                if value is None:
                    return None

                return ctor(
                    url=value["href"],
                    headers=value.get("header", {}),
                    expires_at=(
                        datetime.fromisoformat(value["expires_at"])
                        if "expires_at" in value
                        else None
                    ),
                )

            handles.append(
                BatchHandle(
                    sha256=object["oid"],
                    size=object["size"],
                    upload=create_action(UploadAction, actions.get("upload")),
                    verify=create_action(VerifyAction, actions.get("verify")),
                    download=create_action(DownloadAction, actions.get("download")),
                )
            )

        return handles

    @overload
    def execute_action(self, action: UploadAction, body: bytes) -> None:
        pass

    @overload
    def execute_action(self, action: VerifyAction) -> None:
        pass

    @overload
    def execute_action(self, action: DownloadAction) -> bytes:
        pass

    def execute_action(self, action: Action, body: bytes | None = None):
        request = requests.Request(url=action.url, headers=action.headers, data=body)
        match action:
            case UploadAction():
                request.method = "PUT"
                assert body is not None
            case VerifyAction():
                request.method = "POST"
                assert body is None
            case DownloadAction():
                request.method = "GET"
                assert body is None
            case _:
                raise ValueError()

        response = self._session.send(request.prepare())
        response.raise_for_status()
        if isinstance(action, DownloadAction):
            return response.content
