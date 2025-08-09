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

interface AttestationParams {
  token: string,
  device: string
}

async function generateAttestationParams(videoId: string): Promise<AttestationParams> {
  const RESULT_MESSAGE_TYPE: string = "PMgBwOGHkuhqGFEofJKH";
  const MAX_TRIES: number = 5
  const ATTEMPT_DELAY: number = 1500
  
  const script = document.createElement("script");
  script.innerHTML = `
    (async () => {
      const webPoClient = await window.top["havuokmhhs-0"]?.bevasrs?.wpc();
      let tries = 0;
      for (let tries = 0; tries < ${MAX_TRIES}; ++tries) {
        try {
          const token = await webPoClient.mws({c:"${videoId}",mc:false,me:false});
          window.postMessage({type: "${RESULT_MESSAGE_TYPE}", token, device: yt.config_.DEVICE});
          return;
        } catch (e) {
          const message = e?.message;
          if (typeof message === "string" && message.includes(":notready:")) {
            await new Promise((resolve) => setTimeout(resolve, ${ATTEMPT_DELAY}))
            continue;
          } else {
            throw e;
          }
        }
      }
    })();
  `;

  return Promise.race([
    new Promise<AttestationParams>((resolve) => {
      window.addEventListener("message", message => {
        if (message.data?.type === RESULT_MESSAGE_TYPE &&
          typeof message.data.token === "string" &&
          typeof message.data.device === "string"
        ) {
          script.remove()
          resolve(message.data);
        }
      });

      document.body.appendChild(script)
    }),
    new Promise<AttestationParams>((_, reject) => setTimeout(() => reject("po token minting timed out"), (MAX_TRIES + 1) * ATTEMPT_DELAY))
  ])
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

   const att = await generateAttestationParams(videoId);
   console.log("subrandr: Minted video PO Token \"%s\" (device: \"%s\")", att.token, att.device);
 
  for (const track of playerResponse.captions.playerCaptionsTracklistRenderer.captionTracks) {
    result.push({
      name: track.name.simpleText,
      language: track.languageCode,
      url: track.baseUrl + `&potc=1&pot=${att.token}&c=WEB&${att.device}&fmt=srv3`,
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
