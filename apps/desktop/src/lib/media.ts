// Copied files the app can preview (mirrors src-tauri/src/fileproto.rs).

const IMAGE_EXT = new Set(["png", "jpg", "jpeg", "gif", "webp", "bmp", "avif"]);
const VIDEO_EXT = new Set(["mp4", "m4v", "webm", "mov"]);

/** Phone: a copied image file that's on the computer (fetched on demand, never synced). */
export function remoteImageFile(kind: string, files: string[]): boolean {
  return kind === "file" && files.length > 0 && mediaKind(files[0]) === "image";
}

/** "image" | "video" for files the app can preview (mirrors fileproto.rs). */
export function mediaKind(path: string): "image" | "video" | null {
  const ext = path.split(/[/\\]/).pop()?.split(".").pop()?.toLowerCase() ?? "";
  if (IMAGE_EXT.has(ext)) return "image";
  if (VIDEO_EXT.has(ext)) return "video";
  return null;
}
