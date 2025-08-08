function getYoutubePlayerInfo(): YoutubePlayerInfo | null {
  const video_id = new URLSearchParams(window.location.search).get("v")
  if (!video_id)
    return null

  const movie_player = document.querySelector("#movie_player");
  const video_player = document.querySelector("#movie_player > div:nth-child(1) > video:nth-child(1)") as HTMLVideoElement | null
  if (!movie_player || !video_player)
    return null
  if (video_player.tagName != "VIDEO")
    return null

  const movie_rect = movie_player.getBoundingClientRect()
  const video_rect = video_player.getBoundingClientRect()

  const player_width = movie_rect.width * window.devicePixelRatio
  const player_height = movie_rect.height * window.devicePixelRatio
  const video_width = video_rect.width * window.devicePixelRatio
  const video_height = video_rect.height * window.devicePixelRatio

  return {
    playback_time: Math.floor(video_player.currentTime * 1000),
    viewport: {
      x: movie_rect.x + window.scrollX,
      y: movie_rect.y + window.scrollY,
      width: movie_rect.width,
      height: movie_rect.height,
    },
    frame: {
      player_width: player_width,
      player_height: player_height,
      video_width,
      video_height,
      padding_left: (player_width - video_width) / 2,
      padding_right: (player_width - video_width) / 2,
      padding_top: (player_height - video_height) / 2,
      padding_bottom: (player_height - video_height) / 2,
    },
    video_id
  };
}

function extractPlayerResponse(code: string): any {
  const prefix = "var ytInitialPlayerResponse = "
  const start = code.indexOf(prefix)
  if (start !== -1) {
    const end = code.indexOf(";var meta", start)
    const jsonValue = code.substring(start + prefix.length, end)
    const playerResponse = JSON.parse(jsonValue)
    if (typeof playerResponse != "object")
      return null
    return playerResponse
  }
  return null
}

function extractAttestationResponse(code: string): any {
  const prefix = "ytAtR = "
  const start = code.indexOf(prefix)
  if (start !== -1) {
    const end = code.indexOf(";", start)
    //evil
    const jsonValue = code.substring(start + prefix.length + 1, end - 1)
    .replace(/\\x22/g, "\x22")
    .replace(/\\x7b/g, "\x7b")
    .replace(/\\x5b/g, "\x5b")
    .replace(/\\x7d/g, "\x7d")
    .replace(/\\x3d/g, "\x3d")
    .replace(/\\x5d/g, "\x5d")
    const result = JSON.parse(jsonValue)
    if (typeof result != "object")
      return null
    return result
  }
  return null
}


const TRACK_CACHE: { [key: string]: SubtitleTrack[] } = {}

async function getYoutubeSubtitleTracks(videoId: string) {
  if (videoId in TRACK_CACHE)
    return TRACK_CACHE[videoId];

  const result: SubtitleTrack[] = []

  let playerResponse: any;
  for (const element of document.body.children) {
    if (element.tagName == "SCRIPT") {
      playerResponse = extractPlayerResponse(element.innerHTML)
      if (playerResponse?.videoDetails?.videoId == videoId) {
        console.log("subrandr: Found player response for video id %s in current initial response", playerResponse.videoDetails.videoId)
        break;
      }
    }
  }

  if (!playerResponse) {
    console.log("subrandr: Fetching webpage for video id", videoId)
    const content = await fetch(`https://www.youtube.com/watch?v=${videoId}`).then(r => r.text())
    playerResponse = extractPlayerResponse(content)
    if (playerResponse)
      console.log("subrandr: Retrieved player response for video id", playerResponse.videoDetails.videoId)
  }

  // const challenge: {
  //   globalName: string,
  //   program: string,
  // } = attestationResponse.bgChallenge;

  // console.log(`subrandr: Running pot challenge with interpreter ${challenge.globalName}`);
  // const interpreter = (window as any).wrappedJSObject[challenge.globalName];
  // console.log(interpreter)
  // const syncSnapshot = await interpreter.a(challenge.program, (_a: any, _b: any, _c: any, _d: any) => { }, true, undefined, () => { });
  // const webPoSignalOutput: any[] = [];
  // syncSnapshot({ webPoSignalOutput });
  // console.log(webPoSignalOutput)
  // const integrityTokenResponse = (await fetch('https://jnn-pa.googleapis.com/$rpc/google.internal.waa.v1.Waa/GenerateIT', {
  //   method: 'POST',
  //   headers: {
  //     'Content-Type': 'application/json+protobuf',
  //     'x-goog-api-key': 'AIzaSyDyT5W0Jh49F30Pqqtyfdf7pDLFKLJoAnw',
  //     'x-user-agent': 'grpc-web-javascript/0.1',
  //   },
  //   body: JSON.stringify([ "O43z0dpjhgX20SCx4KAo", webPoSignalOutput ])
  // }));

  // console.log(integrityTokenResponse)

  for (const track of playerResponse.captions.playerCaptionsTracklistRenderer.captionTracks) {
    result.push({
      name: track.name.simpleText,
      language: track.languageCode,
      url: track.baseUrl + "&potc=1&pot=MlR8acEd4RrhUnxU6FsvOBsNkQTadecTRp82MW76vZxjzGr4Ou_iSqH4oCidjwAYglCjTNos0eKHvM2ogWSPfgYIlN0A8uZpSDiGVUZcjTeuyiePkQI=&fmt=srv3&c=WEB&cver=20250807.01.00&cplayer=UNIPLAYER&cos=X11&cplatform=DESKTOp",
      format: "srv3"
    })
  }

  TRACK_CACHE[videoId] = result;

  return result
}

interface YoutubePlayerInfo extends PlayerInfo {
  video_id: string
}

export const YOUTUBE_BACKEND: SiteBackend<YoutubePlayerInfo> = {
  getPlayerInfo: getYoutubePlayerInfo,
  playerInfoDidVideoChange(then, now) {
    return then.video_id != now.video_id
  },
  getSubtitleTracks(info) {
    return getYoutubeSubtitleTracks(info.video_id)
  }
}
