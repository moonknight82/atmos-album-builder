import { useEffect, useState, type DragEvent } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import AlertTriangle from "lucide-react/dist/esm/icons/triangle-alert";
import ArrowDown from "lucide-react/dist/esm/icons/arrow-down";
import ArrowUp from "lucide-react/dist/esm/icons/arrow-up";
import Check from "lucide-react/dist/esm/icons/check";
import CheckCircle2 from "lucide-react/dist/esm/icons/circle-check-big";
import CircleAlert from "lucide-react/dist/esm/icons/circle-alert";
import Disc3 from "lucide-react/dist/esm/icons/disc-3";
import FileAudio from "lucide-react/dist/esm/icons/file-audio";
import FolderOpen from "lucide-react/dist/esm/icons/folder-open";
import GripVertical from "lucide-react/dist/esm/icons/grip-vertical";
import ImagePlus from "lucide-react/dist/esm/icons/image-plus";
import Library from "lucide-react/dist/esm/icons/library";
import LoaderCircle from "lucide-react/dist/esm/icons/loader-circle";
import Music2 from "lucide-react/dist/esm/icons/music-2";
import Pencil from "lucide-react/dist/esm/icons/pencil";
import Play from "lucide-react/dist/esm/icons/play";
import RefreshCw from "lucide-react/dist/esm/icons/refresh-cw";
import RotateCcw from "lucide-react/dist/esm/icons/rotate-ccw";
import ShieldCheck from "lucide-react/dist/esm/icons/shield-check";
import Square from "lucide-react/dist/esm/icons/square";
import Trash2 from "lucide-react/dist/esm/icons/trash-2";
import Video from "lucide-react/dist/esm/icons/video";
import X from "lucide-react/dist/esm/icons/x";
import * as api from "./api";
import type {
  AlbumJob, AlbumMetadata, AlbumStatus, AlbumTrack, ExportProgress, OutputPreview,
} from "./types";

type EditorTab = "metadata" | "tracks" | "output";
const labels: Record<AlbumStatus, string> = {
  needsReview: "Needs review", ready: "Ready", exporting: "Exporting",
  completed: "Completed", conflict: "Conflict", failed: "Failed",
};
const basename = (path: string) => path.split(/[\\/]/).pop() || path;
const duration = (ms: number) => {
  const total = Math.round(ms / 1000), hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60), seconds = total % 60;
  return hours
    ? hours + "h " + String(minutes).padStart(2, "0") + "m"
    : minutes + ":" + String(seconds).padStart(2, "0");
};
const cleanName = (value: string) =>
  value.replace(/[\\/:]/g, " ").replace(/\s+/g, " ").replace(/^[.\s]+|[.\s]+$/g, "");
const autoName = (metadata: AlbumMetadata, fallback: string) => {
  const title = metadata.title.trim() || fallback;
  const stem = metadata.artist.trim() ? metadata.artist.trim() + " - " + title : title;
  return (cleanName(stem) || "Album") + ".mkv";
};
const edited = (album: AlbumJob): AlbumJob => ({
  ...album, approved: false, status: "needsReview", lastError: undefined,
});
const UPDATE_CHECK_INTERVAL = 24 * 60 * 60 * 1000;
const desktopAvailable = () => "__TAURI_INTERNALS__" in window;

export default function App() {
  const [albums, setAlbums] = useState<AlbumJob[]>([]);
  const [selectedId, setSelectedId] = useState<string>();
  const [tab, setTab] = useState<EditorTab>("metadata");
  const [busy, setBusy] = useState("");
  const [notice, setNotice] = useState("");
  const [queueLoaded, setQueueLoaded] = useState(false);
  const [progress, setProgress] = useState<Record<string, ExportProgress>>({});
  const [preview, setPreview] = useState<OutputPreview>();
  const [draggedTrackId, setDraggedTrackId] = useState<string>();
  const [appVersion, setAppVersion] = useState("0.4.0");
  const [updateAvailable, setUpdateAvailable] = useState<Update | null>(null);
  const [updateVisible, setUpdateVisible] = useState(false);
  const [updateChecking, setUpdateChecking] = useState(false);
  const [updateInstalling, setUpdateInstalling] = useState(false);
  const [updateMessage, setUpdateMessage] = useState("");
  const [updateDownloaded, setUpdateDownloaded] = useState(0);
  const [updateTotal, setUpdateTotal] = useState(0);
  const album = albums.find((item) => item.id === selectedId);
  const readyCount = albums.filter((item) => item.approved && item.status !== "completed").length;

  useEffect(() => {
    api.loadQueue().then((loaded) => {
      setAlbums(loaded); setSelectedId(loaded[0]?.id);
    }).catch((error) => setNotice(String(error))).finally(() => setQueueLoaded(true));
  }, []);
  useEffect(() => {
    if (!queueLoaded) return;
    const timer = window.setTimeout(() => api.saveQueue(albums).catch((e) => setNotice(String(e))), 450);
    return () => window.clearTimeout(timer);
  }, [albums, queueLoaded]);
  useEffect(() => {
    let dispose: (() => void) | undefined;
    api.onExportProgress((event) => {
      if (event.albumId === "scan") {
        if (event.stage !== "completed") setBusy(event.message);
        return;
      }
      setProgress((current) => ({ ...current, [event.albumId]: event }));
    }).then((listener) => { dispose = listener; });
    return () => dispose?.();
  }, []);
  useEffect(() => {
    setPreview(undefined);
    if (!album) return;
    const timer = window.setTimeout(() => {
      api.previewOutputPath(album).then(setPreview).catch(() => setPreview(undefined));
    }, 250);
    return () => window.clearTimeout(timer);
  }, [album]);
  useEffect(() => {
    if (!desktopAvailable()) return;
    getVersion().then(setAppVersion).catch(() => undefined);
    const lastCheck = Number(localStorage.getItem("atmos-album-builder.last-update-check") || "0");
    if (Number.isFinite(lastCheck) && Date.now() - lastCheck < UPDATE_CHECK_INTERVAL) return;
    const timer = window.setTimeout(() => void checkForAppUpdate(false), 5000);
    return () => window.clearTimeout(timer);
  }, []);

  const replaceAlbum = (replacement: AlbumJob) =>
    setAlbums((current) => current.map((item) => item.id === replacement.id ? replacement : item));
  const updateAlbum = (id: string, fn: (item: AlbumJob) => AlbumJob) =>
    setAlbums((current) => current.map((item) => item.id === id ? fn(item) : item));

  async function addFolder() {
    const root = await api.chooseRoot();
    if (!root) return;
    setBusy("Scanning folders and reading M4A metadata…"); setNotice("");
    try {
      const found = await api.scanRoot(root);
      const existing = new Set(albums.map((item) => item.sourceFolder));
      const additions = found.filter((item) => !existing.has(item.sourceFolder));
      setAlbums((current) => [...current, ...additions]);
      setSelectedId(additions[0]?.id || found[0]?.id || selectedId);
      setNotice(additions.length
        ? "Added " + additions.length + (additions.length === 1 ? " album" : " albums") + " from " + basename(root) + "."
        : found.length ? "Every discovered album is already in the queue."
        : "No folders containing M4A files were found.");
    } catch (error) { setNotice(String(error)); }
    finally { setBusy(""); }
  }
  function updateMetadata(patch: Partial<AlbumMetadata>) {
    if (!album) return;
    updateAlbum(album.id, (current) => {
      const metadata = { ...current.metadata, ...patch };
      const next = edited({ ...current, metadata });
      return current.fileNameOverridden ? next : { ...next, outputFileName: autoName(metadata, current.folderName) };
    });
  }
  async function inspect(candidate: AlbumJob) {
    setBusy("Checking album readiness…");
    try {
      const result = await api.inspectAlbum(candidate); replaceAlbum(result); return result;
    } catch (error) { setNotice(String(error)); return undefined; }
    finally { setBusy(""); }
  }
  async function chooseImage() {
    if (!album) return;
    const path = await api.chooseImage();
    if (path) await inspect(edited({ ...album, imagePath: path, imageSource: "custom", visualMode: "image" }));
  }
  async function chooseVideo() {
    if (!album) return;
    const path = await api.chooseVideo();
    if (path) await inspect(edited({ ...album, videoPath: path, visualMode: "video" }));
  }
  async function useArtwork() {
    if (album) await inspect(edited({ ...album, visualMode: "image" }));
  }
  async function restoreImage() {
    if (album) await inspect(edited({ ...album, imagePath: undefined, imageSource: "embedded", visualMode: "image" }));
  }
  async function chooseDestination() {
    if (!album) return;
    const path = await api.chooseDestination();
    if (path) replaceAlbum(edited({ ...album, destinationOverride: path }));
  }
  async function reorder(fromId: string, toId: string) {
    if (!album || fromId === toId) return;
    const tracks = [...album.tracks], from = tracks.findIndex((x) => x.id === fromId);
    const to = tracks.findIndex((x) => x.id === toId);
    if (from < 0 || to < 0) return;
    const [moved] = tracks.splice(from, 1); tracks.splice(to, 0, moved);
    await inspect(edited({ ...album, tracks }));
  }
  async function move(index: number, direction: -1 | 1) {
    if (!album) return;
    const target = index + direction;
    if (target < 0 || target >= album.tracks.length) return;
    const tracks = [...album.tracks];
    [tracks[index], tracks[target]] = [tracks[target], tracks[index]];
    await inspect(edited({ ...album, tracks }));
  }
  function updateTrack(id: string, patch: Partial<AlbumTrack>) {
    if (!album) return;
    updateAlbum(album.id, (current) => edited({
      ...current,
      tracks: current.tracks.map((track) => track.id === id ? { ...track, ...patch } : track),
    }));
  }
  async function removeTrack(id: string) {
    if (!album) return;
    const track = album.tracks.find((item) => item.id === id);
    if (!track || !window.confirm("Remove “" + track.fileName + "” from this album?\n\nThe source file will not be deleted.")) return;
    await inspect(edited({ ...album, tracks: album.tracks.filter((item) => item.id !== id) }));
  }
  async function changeExportMode(exportMode: "album" | "individual") {
    if (album) await inspect(edited({ ...album, exportMode }));
  }
  async function approve() {
    if (!album) return;
    const result = await inspect({ ...album, approved: true, status: "ready", lastError: undefined });
    if (!result) return;
    setNotice(result.blockingIssues.length
      ? "This album still needs attention: " + result.blockingIssues.join(" · ")
      : result.metadata.title + " is ready to export.");
  }
  async function approveAll() {
    setBusy("Checking every album…"); setNotice("");
    const checked: AlbumJob[] = []; let count = 0;
    try {
      for (const item of albums) {
        if (item.status === "completed") { checked.push(item); continue; }
        const result = await api.inspectAlbum({ ...item, approved: true, status: "ready", lastError: undefined });
        if (result.approved) count++; checked.push(result);
      }
      setAlbums(checked);
      setNotice(count + (count === 1 ? " album is" : " albums are") + " ready to export.");
    } catch (error) { setNotice(String(error)); }
    finally { setBusy(""); }
  }
  async function runExport(target: AlbumJob, replace: boolean) {
    updateAlbum(target.id, (item) => ({ ...item, status: "exporting", lastError: undefined }));
    try {
      const path = await api.exportAlbum({ ...target, status: "exporting" }, replace);
      updateAlbum(target.id, (item) => ({ ...item, status: "completed", lastError: undefined }));
      return { ok: true as const, path };
    } catch (error) {
      const message = String(error), conflict = message.includes("CONFLICT:");
      updateAlbum(target.id, (item) => ({
        ...item, status: conflict ? "conflict" : "failed",
        lastError: message.replace(/^.*CONFLICT:\s*/, ""),
      }));
      return { ok: false as const, error: message, conflict };
    }
  }
  async function exportOne(replace = false) {
    if (!album) return;
    if (!album.approved) { setNotice("Approve this album before exporting it."); return; }
    if (preview?.exists && !replace) {
      updateAlbum(album.id, (item) => ({ ...item, status: "conflict", lastError: "Destination already exists: " + preview.path }));
      setNotice("The destination exists. Change the filename or choose Replace existing."); return;
    }
    if (replace && !window.confirm(album.exportMode === "individual"
      ? "Replace " + (preview?.conflictCount || "the") + " existing individual file(s)?\n\n" + (preview?.path || album.sourceFolder)
      : "Replace the existing file?\n\n" + (preview?.path || album.outputFileName))) return;
    setBusy("Exporting " + album.metadata.title + "…"); setNotice("");
    const result = await runExport(album, replace); setBusy("");
    setNotice(result.ok ? (album.exportMode === "individual" ? "Created individual MKVs in " : "Created ") + result.path
      : result.conflict ? "The destination already exists. Replace it explicitly or change the filename."
      : result.error);
  }
  async function exportAll() {
    const pending = albums.filter((item) => item.approved && item.status !== "completed");
    if (!pending.length) { setNotice("There are no approved albums waiting to export."); return; }
    setNotice(""); let done = 0, failed = 0, conflicts = 0;
    for (let index = 0; index < pending.length; index++) {
      const current = pending[index]; setSelectedId(current.id);
      setBusy("Exporting " + (index + 1) + " of " + pending.length + ": " + current.metadata.title);
      try {
        const target = await api.previewOutputPath(current);
        if (target.exists) {
          conflicts++;
          updateAlbum(current.id, (item) => ({ ...item, status: "conflict", lastError: "Destination already exists: " + target.path }));
          continue;
        }
      } catch (error) {
        failed++;
        updateAlbum(current.id, (item) => ({ ...item, status: "failed", lastError: String(error) }));
        continue;
      }
      const result = await runExport(current, false);
      if (result.ok) done++; else if (result.conflict) conflicts++; else failed++;
    }
    setBusy("");
    setNotice(done + " exported" + (conflicts ? " · " + conflicts + " need conflict review" : "") + (failed ? " · " + failed + " failed" : "") + ".");
  }
  async function cancel() {
    const target = albums.find((item) => item.status === "exporting");
    if (target) { await api.cancelExport(target.id); setNotice("Cancelling export…"); }
  }
  async function checkForAppUpdate(showWhenCurrent = true) {
    if (updateChecking || updateInstalling) return;
    if (!desktopAvailable()) {
      setUpdateMessage("Update checks are available in the installed desktop app.");
      setUpdateVisible(true);
      return;
    }
    if (showWhenCurrent) setUpdateVisible(true);
    setUpdateChecking(true);
    setUpdateMessage("Checking GitHub for a signed update…");
    setUpdateDownloaded(0);
    setUpdateTotal(0);
    if (updateAvailable) {
      await updateAvailable.close();
      setUpdateAvailable(null);
    }
    try {
      const candidate = await check({ timeout: 15000 });
      localStorage.setItem("atmos-album-builder.last-update-check", String(Date.now()));
      if (candidate) {
        setUpdateAvailable(candidate);
        setUpdateVisible(true);
        setUpdateMessage("Version " + candidate.version + " is ready to install.");
      } else {
        setUpdateMessage("Atmos Album Builder " + appVersion + " is up to date.");
      }
    } catch (error) {
      if (showWhenCurrent) {
        setUpdateVisible(true);
        setUpdateMessage("Could not check for updates: " + String(error));
      }
    } finally {
      setUpdateChecking(false);
    }
  }
  async function installAppUpdate() {
    if (!updateAvailable || updateInstalling) return;
    setUpdateInstalling(true);
    setUpdateMessage("Downloading and verifying the signed update…");
    try {
      await updateAvailable.downloadAndInstall((event) => {
        if (event.event === "Started") {
          setUpdateTotal(event.data.contentLength || 0);
          setUpdateDownloaded(0);
        } else if (event.event === "Progress") {
          setUpdateDownloaded((current) => current + event.data.chunkLength);
        } else if (event.event === "Finished") {
          setUpdateMessage("Update installed. Restarting Atmos Album Builder…");
        }
      });
      await relaunch();
    } catch (error) {
      setUpdateMessage("The update was not installed: " + String(error));
      setUpdateInstalling(false);
    }
  }
  async function closeUpdateDialog() {
    if (updateChecking || updateInstalling) return;
    setUpdateVisible(false);
    if (updateAvailable) await updateAvailable.close();
    setUpdateAvailable(null);
  }
  function remove(id: string) {
    const target = albums.find((item) => item.id === id);
    if (!target || !window.confirm("Remove “" + (target.metadata.title || target.folderName) + "” from the queue?\n\nSource and exported files will not be deleted.")) return;
    const remaining = albums.filter((item) => item.id !== id);
    setAlbums(remaining); if (selectedId === id) setSelectedId(remaining[0]?.id);
  }
  const selectedProgress = album ? progress[album.id] : undefined;

  return (
    <div className="app-shell">
      <header className="topbar">
        <div className="brand"><div className="brand-mark"><Disc3 /></div><div>
          <strong>Atmos Album Builder</strong>
          <span>Lossless audio · chaptered MKV · 1080p artwork</span>
        </div></div>
        <div className="top-actions">
          {busy && <span className="busy-label"><LoaderCircle className="spin" /> {busy}</span>}
          {albums.some((item) => item.status === "exporting") ? (
            <button className="secondary danger" onClick={cancel}><Square /> Cancel export</button>
          ) : (<>
            <button className={"icon-button update-button " + (updateAvailable ? "available" : "")} onClick={() => void checkForAppUpdate(true)} disabled={updateChecking} title={"Check for updates · v" + appVersion}><RefreshCw className={updateChecking ? "spin" : ""} /></button>
            <button className="secondary" onClick={approveAll} disabled={!albums.length || Boolean(busy)}><ShieldCheck /> Approve all valid</button>
            <button className="primary" onClick={exportAll} disabled={!readyCount || Boolean(busy)}><Play /> Export all ready {readyCount > 0 && <b>{readyCount}</b>}</button>
          </>)}
        </div>
      </header>
      {notice && <div className="notice"><span>{notice}</span><button onClick={() => setNotice("")}><X /></button></div>}
      <main className="workspace">
        <aside className="sidebar">
          <div className="sidebar-head"><div><span className="eyebrow">Review queue</span><h1>{albums.length} albums</h1></div>
            <button className="icon-button accent" onClick={addFolder} disabled={Boolean(busy)} title="Add a root folder"><FolderOpen /></button>
          </div>
          {!albums.length ? <button className="empty-queue" onClick={addFolder}><div><Library /></div><strong>Add your album library</strong><span>Every subfolder containing M4A files becomes an album.</span></button>
          : <div className="album-list">{albums.map((item) => {
            const p = progress[item.id];
            return <button key={item.id} className={"album-row " + (selectedId === item.id ? "selected" : "")} onClick={() => setSelectedId(item.id)}>
              <div className="row-art">{item.imagePath ? <img src={convertFileSrc(item.imagePath)} /> : <Music2 />}</div>
              <div className="row-copy"><strong>{item.metadata.title || item.folderName}</strong><span>{item.metadata.artist || "Unknown artist"}</span>
                {item.status === "exporting" && p ? <div className="mini-progress"><i style={{ width: String(p.percent) + "%" }} /></div>
                : <small>{item.tracks.length} tracks · {duration(item.totalDurationMs)}</small>}
              </div><span className={"status-dot " + item.status} title={labels[item.status]} />
            </button>;
          })}</div>}
        </aside>
        <section className="content">
          {!album ? <div className="welcome"><div className="welcome-visual"><Disc3 /><i /></div><span className="eyebrow">Local, private, lossless</span><h2>Turn Atmos tracks into polished album MKVs.</h2><p>Review metadata, chapters, artwork, and destinations before a safe batch export.</p><button className="primary large" onClick={addFolder}><FolderOpen /> Choose a library folder</button></div>
          : <>
            <div className="album-header"><div><div className="header-kicker"><span className={"status-pill " + album.status}>{album.status === "completed" && <CheckCircle2 />}{(album.status === "conflict" || album.status === "failed") && <AlertTriangle />}{labels[album.status]}</span><span>{album.folderName}</span></div><h2>{album.metadata.title || "Untitled album"}</h2><p>{album.sourceFolder}</p></div>
              <div className="header-actions"><button className="icon-button" onClick={() => remove(album.id)} disabled={album.status === "exporting"}><Trash2 /></button>
                {album.approved ? <button className="secondary" onClick={() => replaceAlbum({ ...album, approved: false, status: "needsReview" })}><Pencil /> Reopen review</button>
                : <button className="secondary approve" onClick={approve} disabled={Boolean(busy)}><Check /> Approve album</button>}
                {album.status === "conflict" ? <button className="primary warning" onClick={() => exportOne(true)} disabled={Boolean(busy)}><RefreshCw /> Replace existing</button>
                : <button className="primary" onClick={() => exportOne(false)} disabled={!album.approved || album.status === "exporting" || Boolean(busy)}><Play /> {album.exportMode === "individual" ? "Export tracks" : "Export album"}</button>}
              </div>
            </div>
            {album.status === "exporting" && selectedProgress && <div className="export-progress"><div><span>{selectedProgress.message}</span><strong>{selectedProgress.percent}%</strong></div><div><i style={{ width: String(selectedProgress.percent) + "%" }} /></div></div>}
            {(album.blockingIssues.length > 0 || album.warnings.length > 0 || album.lastError) && <div className="issues">
              {album.lastError && <div className="issue error"><CircleAlert /><span>{album.lastError}</span></div>}
              {album.blockingIssues.map((issue) => <div className="issue error" key={issue}><CircleAlert /><span>{issue}</span></div>)}
              {album.warnings.map((warning) => <div className="issue warning" key={warning}><AlertTriangle /><span>{warning}</span></div>)}
            </div>}
            <nav className="tabs"><button className={tab === "metadata" ? "active" : ""} onClick={() => setTab("metadata")}>Album details</button><button className={tab === "tracks" ? "active" : ""} onClick={() => setTab("tracks")}>Tracks & chapters <b>{album.tracks.length}</b></button><button className={tab === "output" ? "active" : ""} onClick={() => setTab("output")}>Output</button></nav>
            <div className="editor">
              {tab === "metadata" && <MetadataEditor album={album} updateMetadata={updateMetadata} chooseImage={chooseImage} chooseVideo={chooseVideo} useArtwork={useArtwork} restoreImage={restoreImage} />}
              {tab === "tracks" && <TrackEditor album={album} dragged={draggedTrackId} setDragged={setDraggedTrackId} reorder={reorder} move={move} updateTrack={updateTrack} removeTrack={removeTrack} />}
              {tab === "output" && <OutputEditor album={album} preview={preview} chooseDestination={chooseDestination} changeExportMode={changeExportMode} update={(patch) => updateAlbum(album.id, (current) => edited({ ...current, ...patch }))} />}
            </div>
          </>}
        </section>
      </main>
      {updateVisible && <div className="modal-layer" role="presentation">
        <button className="modal-backdrop" aria-label="Close application update" disabled={updateChecking || updateInstalling} onClick={() => void closeUpdateDialog()} />
        <div className="update-dialog" role="dialog" aria-modal="true" aria-label="Application update">
          <header><div><span className="eyebrow">Atmos Album Builder</span><h2>{updateAvailable ? "Update to " + updateAvailable.version : "Application updates"}</h2></div><button aria-label="Close" disabled={updateChecking || updateInstalling} onClick={() => void closeUpdateDialog()}><X /></button></header>
          <section><p className={(updateMessage.includes("Could not") || updateMessage.includes("not installed")) ? "update-error" : ""}>{updateMessage}</p>
            {updateAvailable?.body && <div className="release-notes"><strong>What’s new</strong><p>{updateAvailable.body}</p></div>}
            {updateInstalling && <div className="update-download"><div><i style={{ width: updateTotal ? Math.min(100, Math.round(updateDownloaded / updateTotal * 100)) + "%" : "18%" }} /></div><span>{updateTotal ? Math.min(100, Math.round(updateDownloaded / updateTotal * 100)) + "%" : "Working…"}</span></div>}
            <small>Updates come from the public GitHub release and are cryptographically verified before installation.</small>
          </section>
          <footer><span>Installed version {appVersion}</span><div><button className="secondary" disabled={updateChecking || updateInstalling} onClick={() => void closeUpdateDialog()}>Not now</button>{updateAvailable ? <button className="primary" disabled={updateInstalling} onClick={() => void installAppUpdate()}>{updateInstalling ? "Installing…" : "Install and restart"}</button> : <button className="primary" disabled={updateChecking} onClick={() => void checkForAppUpdate(true)}>{updateChecking ? "Checking…" : "Check again"}</button>}</div></footer>
        </div>
      </div>}
    </div>
  );
}

function Field(props: { label: string; value: string; onChange: (v: string) => void; wide?: boolean; placeholder?: string }) {
  return <label className={props.wide ? "wide" : ""}><span>{props.label}</span><input value={props.value} placeholder={props.placeholder} onChange={(e) => props.onChange(e.target.value)} /></label>;
}

function MetadataEditor({ album, updateMetadata, chooseImage, chooseVideo, useArtwork, restoreImage }: {
  album: AlbumJob; updateMetadata: (p: Partial<AlbumMetadata>) => void; chooseImage: () => void; chooseVideo: () => void; useArtwork: () => void; restoreImage: () => void;
}) {
  const animated = album.visualMode === "video" && Boolean(album.videoPath);
  return <div className="details-grid">
    <div className="image-panel"><div className="image-frame">
      {animated ? <video src={convertFileSrc(album.videoPath!)} autoPlay loop muted playsInline /> : album.imagePath ? <img src={convertFileSrc(album.imagePath)} /> : <div className="missing-image"><ImagePlus /><span>No usable artwork</span></div>}
      <span className="image-badge">{animated ? "Looping animation" : album.imageSource === "custom" ? "Custom image" : "Embedded artwork"}</span>
    </div><p>{animated ? "The animation loops silently until the audio ends. Its own audio is ignored, and the picture is fitted inside 1920×1080." : "The image is fitted inside a 1920×1080 frame with black padding. It is the only stream that gets encoded."}</p>
      <div className="image-actions"><button className="secondary" onClick={chooseImage}><ImagePlus /> Choose image</button><button className="secondary" onClick={chooseVideo}><Video /> Choose animation</button>{animated ? <button className="text-button" onClick={useArtwork}><RotateCcw /> Use artwork</button> : album.imageSource === "custom" && <button className="text-button" onClick={restoreImage}><RotateCcw /> Use embedded</button>}</div>
    </div>
    <div className="form-card"><div className="section-title"><div><span className="eyebrow">MKV metadata</span><h3>Album identity</h3></div><small>Read from the sources, fully editable</small></div>
      <div className="form-grid"><Field label="Album title" value={album.metadata.title} onChange={(title) => updateMetadata({ title })} wide /><Field label="Album artist" value={album.metadata.artist} onChange={(artist) => updateMetadata({ artist })} wide /><Field label="Year or release date" value={album.metadata.date} onChange={(date) => updateMetadata({ date })} placeholder="2026" /><Field label="Genre" value={album.metadata.genre} onChange={(genre) => updateMetadata({ genre })} placeholder="Optional" /><label className="wide"><span>Comment</span><textarea value={album.metadata.comment} placeholder="Optional" onChange={(e) => updateMetadata({ comment: e.target.value })} /></label></div>
      <div className="tag-note"><ShieldCheck /><span>The creator is written as <b>artist</b>, <b>album_artist</b>, and <b>author</b> for broad player compatibility.</span></div>
    </div>
  </div>;
}

function TrackEditor({ album, dragged, setDragged, reorder, move, updateTrack, removeTrack }: {
  album: AlbumJob; dragged?: string; setDragged: (id?: string) => void; reorder: (a: string, b: string) => void; move: (i: number, d: -1 | 1) => void; updateTrack: (id: string, p: Partial<AlbumTrack>) => void; removeTrack: (id: string) => void;
}) {
  let cursor = 0;
  return <div className="track-section"><div className="section-title"><div><span className="eyebrow">Playback order</span><h3>Tracks become chapters</h3></div><small>Drag rows or use the arrow controls</small></div><div className="track-list">
    {album.tracks.map((track, index) => { const startsAt = cursor; cursor += track.durationMs; return <div key={track.id} draggable className={"track-row " + (dragged === track.id ? "dragging" : "")}
      onDragStart={(e: DragEvent) => { e.dataTransfer.effectAllowed = "move"; setDragged(track.id); }} onDragEnd={() => setDragged(undefined)}
      onDragOver={(e) => { e.preventDefault(); e.dataTransfer.dropEffect = "move"; }} onDrop={(e) => { e.preventDefault(); if (dragged) reorder(dragged, track.id); setDragged(undefined); }}>
      <GripVertical className="grip" /><b className="track-number">{index + 1}</b>
      <div className="track-copy"><div className="track-copy-title"><strong>{track.fileName}</strong><span className="codec-badge">{track.codec.toUpperCase()}{track.profile ? " · " + track.profile : ""}</span></div><span>{duration(startsAt)} · {track.sampleRate ? track.sampleRate / 1000 + " kHz" : "—"} · {track.channelLayout || track.channels + " channels"}</span></div>
      <label className="chapter-field"><span>Chapter title</span><input value={track.chapterTitle} onChange={(e) => updateTrack(track.id, { chapterTitle: e.target.value })} /></label>
      <div className="track-actions"><button title="Move up" onClick={() => move(index, -1)} disabled={index === 0}><ArrowUp /></button><button title="Move down" onClick={() => move(index, 1)} disabled={index === album.tracks.length - 1}><ArrowDown /></button><button className="remove-track" title="Remove from album" onClick={() => removeTrack(track.id)}><Trash2 /></button></div>
    </div>; })}
  </div></div>;
}

function OutputEditor({ album, preview, chooseDestination, changeExportMode, update }: {
  album: AlbumJob; preview?: OutputPreview; chooseDestination: () => void; changeExportMode: (mode: "album" | "individual") => void; update: (p: Partial<AlbumJob>) => void;
}) {
  return <div className="output-layout"><div className="form-card output-card"><div className="section-title"><div><span className="eyebrow">Destination</span><h3>Output file</h3></div><small>The source files are never changed</small></div>
    <div className="export-mode"><button className={album.exportMode !== "individual" ? "selected" : ""} onClick={() => changeExportMode("album")}><Disc3 /><span><b>One album MKV</b><small>Tracks become chapters in one file</small></span></button><button className={album.exportMode === "individual" ? "selected" : ""} onClick={() => changeExportMode("individual")}><FileAudio /><span><b>Individual MKVs</b><small>One verified MKV for every retained track</small></span></button></div>
    <div className="form-grid">{album.exportMode !== "individual" && <label className="wide"><span>MKV filename</span><div className="compound-field"><input value={album.outputFileName} onChange={(e) => update({ outputFileName: e.target.value, fileNameOverridden: true })} />{album.fileNameOverridden && <button className="secondary" onClick={() => update({ outputFileName: autoName(album.metadata, album.folderName), fileNameOverridden: false })}><RefreshCw /> Auto</button>}</div></label>}
    <label className="wide"><span>Export folder</span><div className="compound-field"><input value={album.destinationOverride || album.sourceFolder} onChange={(e) => update({ destinationOverride: e.target.value })} /><button className="secondary" onClick={chooseDestination}><FolderOpen /> Choose</button></div></label></div>
    {album.exportMode === "individual" && <p className="individual-note">Files are named from the approved order and chapter titles: <b>01 - Track title.mkv</b>, <b>02 - Track title.mkv</b>, and so on.</p>}
    {album.destinationOverride && <button className="text-button reset-destination" onClick={() => update({ destinationOverride: undefined })}><RotateCcw /> Use the source album folder</button>}
  </div>
  <div className={"path-preview " + (preview?.exists ? "conflict" : "")}><div>{preview?.exists ? <AlertTriangle /> : <FileAudio />}<span>{preview?.exists ? (album.exportMode === "individual" ? preview.conflictCount + " existing file(s)" : "Existing destination") : (album.exportMode === "individual" ? album.tracks.length + " individual MKVs" : "Final MKV")}</span></div><strong>{preview?.path || "Checking output path…"}</strong><p>{preview?.exists ? "Batch export leaves existing files untouched until you explicitly replace them or change the destination." : album.exportMode === "individual" ? "Every track is written to a temporary MKV, verified, and then committed separately." : "The MKV is written to a temporary file, verified, and then committed here."}</p></div>
  <div className="preservation-card"><div><ShieldCheck /></div><section><span className="eyebrow">Preservation contract</span><h3>Audio packets must match.</h3><p>The app stream-copies the audio and compares SHA-256 packet hashes before committing the MKV. Only the {album.visualMode === "video" ? "looping animation" : "1080p still image"} is encoded.</p></section></div>
  </div>;
}
