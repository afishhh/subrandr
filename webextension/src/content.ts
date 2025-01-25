import { YOUTUBE_BACKEND } from "./sites/youtube"
import type { WorkerMessage } from "./worker"

(async () => {
  const SITES: { [key: string]: SiteBackend<PlayerInfo> | undefined } = {
    "www.youtube.com": YOUTUBE_BACKEND
  }

  function createCanvas() {
    const canvas = document.createElement("canvas")
    canvas.id = "subrandr-overlay"
    canvas.style.position = "absolute"
    document.body.append(canvas)
    return canvas
  }

  function getCanvas() {
    return document.getElementById("subrandr-overlay") as HTMLCanvasElement ?? createCanvas()
  }

  function pickTrack(tracks: SubtitleTrack[]) {
    return tracks.find(t => t.language == "ja") || tracks[0]
  }

  class RenderWorker {
    private _worker: Worker
    private _outgoingRequests: Map<number, (value: any) => void> = new Map
    private _nextRequestId = 0

    private _postMessage(message: WorkerMessage, transfer?: Array<Transferable>) {
      this._worker.postMessage(message, transfer ?? [])
    }

    async sendRequest(message: WorkerMessage, transfer?: Array<Transferable>): Promise<any> {
      return new Promise(resolve => {
        message.id = this._nextRequestId
        this._outgoingRequests.set(this._nextRequestId, resolve)
        this._nextRequestId += 1
        this._postMessage(message, transfer)
      })
    }

    private constructor(worker: Worker, resolve: (value: RenderWorker) => void) {
      this._worker = worker

      this._worker.onmessage = (event) => {
        if (event.data == "ready")
          resolve(this)
        else {
          const resolve = this._outgoingRequests.get(event.data.id)!!
          this._outgoingRequests.delete(event.data.id)
          resolve(event.data)
        }
      }
    }

    static async start(): Promise<RenderWorker> {
      let fullUrl = browser.runtime.getURL("worker.js")
      fullUrl += "?wasm=" + encodeURIComponent(browser.runtime.getURL("subrandr.wasm"))
      fullUrl += "&js=" + encodeURIComponent(browser.runtime.getURL("subrandr.js"))

      const worker = new Worker(fullUrl, {
        type: "module"
      })

      return new Promise(resolve => new RenderWorker(worker, resolve))
    }
  }

  interface PlayingState {
    type: "playing",
    track: SubtitleTrack,
    playerInfo: PlayerInfo
  }

  interface LoadingState {
    type: "loading"
    track: SubtitleTrack
  }

  interface IdleState {
    type: "idle"
  }

  type State = PlayingState | LoadingState | IdleState

  const site = SITES[document.location.hostname]
  if (!site) {
    console.error("subrandr site not recognized:", document.location.hostname)
    return;
  }

  let playerInfo: PlayerInfo | null = null;
  let subtitleTracks: SubtitleTrack[] = []
  let state: State = { type: "idle" }
  let worker: RenderWorker | null = null

  const updateState = async () => {
    const newInfo = site.getPlayerInfo()
    let newSubs = false;
    if (newInfo) {
      newSubs = playerInfo ? site.playerInfoDidVideoChange(playerInfo, newInfo) : true
      subtitleTracks = []
    }

    const oldInfo = playerInfo
    playerInfo = newInfo

    if (newInfo && newSubs) {
      subtitleTracks = await site.getSubtitleTracks(newInfo)

      const track = pickTrack(subtitleTracks)
      const subBytes = await fetch(track.url).then(r => r.bytes())

      if (worker === null)
        worker = await RenderWorker.start()

      state = { type: "loading", track }
      await worker.sendRequest({
        type: "load-subtitles",
        text: subBytes,
      }, [subBytes.buffer])
      state = { type: "playing", track, playerInfo: newInfo }
    } else if (!newInfo && oldInfo) {
      state = { type: "idle" }
      getCanvas().style.display = "none"
    }
  }

  // const observer = new MutationObserver(updateState)

  // if(document.body)
  //   observer.observe(document.body, {
  //     subtree: true,
  //     childList: true
  //   })
  // else
  //   document.addEventListener("DOMContentLoaded", () => {
  //     observer.observe(document.body, {
  //       subtree: true,
  //       childList: true
  //     })
  //   })

  const onframe = async () => {
    updateState();

    if (state.type == "playing") {
      const pi = site.getPlayerInfo()!!
      const canvas = getCanvas()

      const response = await worker!!.sendRequest({
        type: "render",
        player_width: pi.frame.player_width,
        player_height: pi.frame.player_height,
        ctx: {
          dpi: 72 * window.devicePixelRatio,
          video_width: pi.frame.video_width,
          video_height: pi.frame.video_height,
          padding_left: pi.frame.padding_left,
          padding_right: pi.frame.padding_right,
          padding_top: pi.frame.padding_top,
          padding_bottom: pi.frame.padding_bottom,
        },
        playback_time: pi.playback_time,
      })
      const bitmap = response.bitmap as ImageBitmap

      canvas.style.left = pi.viewport.x + "px"
      canvas.style.top = pi.viewport.y + "px"
      canvas.width = Math.ceil(pi.viewport.width)
      canvas.height = Math.ceil(pi.viewport.height)
      canvas.style.width = pi.viewport.width + "px"
      canvas.style.height = pi.viewport.height + "px"
      canvas.style.pointerEvents = "none"

      const bitmapContext = canvas.getContext("bitmaprenderer")!!
      bitmapContext.transferFromImageBitmap(bitmap)
      bitmap.close()

      canvas.style.display = "block"
    }

    requestAnimationFrame(onframe)
  };

  requestAnimationFrame(onframe)
})()
