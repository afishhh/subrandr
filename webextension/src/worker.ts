import type * as S from "subrandr"

export type LoadSubtitlesMessage = {
  type: "load-subtitles"
  text: string | Uint8Array
}


export type RenderMessage = {
  type: "render"
  player_width: number,
  player_height: number,
  ctx: S.SubtitleContext,
  playback_time: number
}

export type WorkerMessage = (RenderMessage | LoadSubtitlesMessage) & {
  id?: any
}

const subrandrUrl = new URL(import.meta.url).searchParams.get("js")!!
const wasmUrl = new URL(import.meta.url).searchParams.get("wasm")!!
const { initStreaming, Subtitles, Renderer, Framebuffer, Font } = await import(subrandrUrl) as typeof S

await initStreaming(fetch(wasmUrl), {
  initial_log_filter: "error"
});

const notoSans = fetch("https://fishhh.dev/files/cors/NotoSansJP-VariableFont_wght.ttf")
  .then(r => r.bytes())
  .then(b => new Font(b))
const notoSansItalic = fetch("https://fishhh.dev/files/cors/NotoSans-Italic-VariableFont_wdth,wght.ttf")
  .then(r => r.bytes())
  .then(b => new Font(b))

const fb = new Framebuffer(0, 0)
let subtitles: S.Subtitles | null = null
let renderer: S.Renderer | null = null

self.onmessage = async (event: MessageEvent<WorkerMessage>) => {
  switch (event.data.type) {
    case "load-subtitles":
      if(renderer) {
        renderer.destroy();
        renderer = null
      }
      if(subtitles) {
        subtitles.destroy();
        subtitles = null
      }

      console.log("reloading subs")
      
      subtitles = Subtitles.parseFromString(event.data.text)
      renderer = new Renderer()
      
      renderer.addFont(
        "Noto Sans JP",
        "variadic",
        false,
        await notoSans
      );

      renderer.addFont(
        "Noto Sans Italic",
        "variadic",
        true,
        await notoSansItalic
      );

      postMessage({ id: event.data.id })
      break;
    case "render":
      const params = event.data

      fb.resize(Math.ceil(params.player_width), Math.ceil(params.player_height))
      renderer!!.render(params.ctx, fb, subtitles!!, params.playback_time)

      const bitmap = await fb.imageBitmap()
      postMessage({ id: event.data.id, bitmap }, { transfer: [bitmap] })
      break;
    default:
      throw Error(`render worker received unknown message type ${(event.data as any).type}`)
  }
}
self.postMessage("ready")
