export type AlbumStatus =
  | "needsReview"
  | "ready"
  | "exporting"
  | "completed"
  | "conflict"
  | "failed";

export interface AlbumMetadata {
  title: string;
  artist: string;
  date: string;
  genre: string;
  comment: string;
}

export interface AlbumTrack {
  id: string;
  path: string;
  fileName: string;
  durationMs: number;
  chapterTitle: string;
  discNumber?: number;
  trackNumber?: number;
  codec: string;
  profile: string;
  sampleRate: number;
  channels: number;
  channelLayout: string;
  codecTag: string;
  extradataSize: number;
  extradataFingerprint: string;
  album: string;
  albumArtist: string;
  artist: string;
  date: string;
  genre: string;
  comment: string;
  hasEmbeddedCover: boolean;
}

export interface AlbumJob {
  id: string;
  sourceFolder: string;
  folderName: string;
  tracks: AlbumTrack[];
  metadata: AlbumMetadata;
  imagePath?: string;
  imageSource: "embedded" | "custom";
  visualMode: "image" | "video";
  videoPath?: string;
  outputFileName: string;
  exportMode: "album" | "individual";
  fileNameOverridden: boolean;
  destinationOverride?: string;
  approved: boolean;
  status: AlbumStatus;
  warnings: string[];
  blockingIssues: string[];
  totalDurationMs: number;
  lastError?: string;
}

export interface OutputPreview {
  path: string;
  exists: boolean;
  paths: string[];
  conflictCount: number;
}

export interface ExportProgress {
  albumId: string;
  stage: string;
  percent: number;
  message: string;
}
