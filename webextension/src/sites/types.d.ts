interface PlayerViewport {
  x: number,
  y: number,
  width: number,
  height: number,
}

interface PlayerFrameInfo {
  player_width: number,
  player_height: number,
  video_width: number,
  video_height: number,
  padding_left?: number,
  padding_right?: number,
  padding_top?: number,
  padding_bottom?: number,
}

interface PlayerInfo {
  playback_time: number,
  viewport: PlayerViewport,
  frame: PlayerFrameInfo,
}

interface SubtitleTrack {
  name: string,
  language?: string,
  url: string,
  format: "srv3" | "ass"
}

interface SiteBackend<P extends PlayerInfo> {
  getPlayerInfo(): P | null;
  playerInfoDidVideoChange(then: P, now: P)
  getSubtitleTracks(info: P): Promise<SubtitleTrack[]>;
}
