import { invoke } from "@tauri-apps/api/core";
import type { AlbumJob, ExportProgress, OutputPreview } from "./types";

const inTauri = () => "__TAURI_INTERNALS__" in window;

export async function chooseRoot(): Promise<string | undefined> {
  if (!inTauri()) return undefined;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const result = await open({
    directory: true,
    multiple: false,
    title: "Choose a folder containing album folders",
  });
  return typeof result === "string" ? result : undefined;
}

export async function chooseImage(): Promise<string | undefined> {
  if (!inTauri()) return undefined;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const result = await open({
    directory: false,
    multiple: false,
    title: "Choose the album video image",
    filters: [
      {
        name: "Images",
        extensions: ["jpg", "jpeg", "png", "webp", "tif", "tiff", "bmp"],
      },
    ],
  });
  return typeof result === "string" ? result : undefined;
}

export async function chooseVideo(): Promise<string | undefined> {
  if (!inTauri()) return undefined;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const result = await open({
    directory: false,
    multiple: false,
    title: "Choose a looping album animation",
    filters: [
      {
        name: "Videos",
        extensions: ["mp4", "mov", "m4v"],
      },
    ],
  });
  return typeof result === "string" ? result : undefined;
}

export async function chooseDestination(): Promise<string | undefined> {
  if (!inTauri()) return undefined;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const result = await open({
    directory: true,
    multiple: false,
    title: "Choose an export folder for this album",
  });
  return typeof result === "string" ? result : undefined;
}

export const scanRoot = (root: string) =>
  invoke<AlbumJob[]>("scan_root", { root });

export const inspectAlbum = (album: AlbumJob) =>
  invoke<AlbumJob>("inspect_album", { album });

export const previewOutputPath = (album: AlbumJob) =>
  invoke<OutputPreview>("preview_output_path", { album });

export const exportAlbum = (album: AlbumJob, replace = false) =>
  invoke<string>("export_album", { album, replace });

export const cancelExport = (albumId: string) =>
  invoke<void>("cancel_export", { albumId });

export const loadQueue = () =>
  inTauri() ? invoke<AlbumJob[]>("load_queue") : Promise.resolve([]);

export const saveQueue = (albums: AlbumJob[]) =>
  inTauri() ? invoke<void>("save_queue", { albums }) : Promise.resolve();

export async function onExportProgress(
  handler: (progress: ExportProgress) => void,
): Promise<() => void> {
  if (!inTauri()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<ExportProgress>("export-progress", (event) =>
    handler(event.payload),
  );
}
