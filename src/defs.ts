// Declarative definitions of every operation. The tool view renders forms
// from these, so adding an operation means adding an entry here, a backend
// `Operation` variant, and its strings in the locale files.

export type Section = "video" | "audio" | "image" | "pdf" | "disc" | "download" | "recorder" | "archive";

export type FieldType = "select" | "number" | "text" | "checkbox" | "file" | "time";

export interface Field {
  key: string;
  type: FieldType;
  /** Static options, or a named dynamic source resolved at render time. */
  options?: string[] | "presets" | "accel";
  default?: string | number | boolean;
  min?: number;
  max?: number;
  step?: number;
  /** Shown only when this returns true for the current params. */
  showIf?: (p: Params) => boolean;
  /** For `select`: allow an empty "default/none" entry. */
  optional?: boolean;
  /** Translation key for a hint below the field. */
  hintKey?: string;
}

export type Params = Record<string, string | number | boolean | undefined>;

export interface OpDef {
  id: string;
  section: Section;
  /** Category sent to backend validation for file inputs. */
  category?: "video" | "audio" | "image" | "pdf" | "archive" | "media";
  input: "files" | "folder" | "none";
  /** All inputs become one job (join/merge/mix) instead of one job per file. */
  combine?: boolean;
  fields: Field[];
  /** Param holding the output format, shown in the queue. */
  formatParam?: string;
  /** Fixed output label when there is no format param. */
  outputLabel?: string;
  /** A checkbox the user must tick (authorization / acknowledgement). */
  confirmParam?: string;
  /** Translation key of a caveat shown above the form. */
  noticeKey?: string;
}

const VIDEO_OUT = ["mp4", "mkv", "mov", "avi", "webm", "wmv", "flv", "mpg", "3gp", "m4v", "ts", "gif"];
const AUDIO_OUT = ["mp3", "m4a", "aac", "ogg", "opus", "flac", "wav", "wma", "aiff", "ac3"];
const IMAGE_OUT = ["png", "jpg", "webp", "bmp", "gif", "tiff", "ico", "tga", "qoi", "heic", "avif"];
const lossy = (p: Params) => !["flac", "wav", "aiff"].includes(String(p.format));

const tagFields: Field[] = [
  { key: "title", type: "text" },
  { key: "artist", type: "text" },
  { key: "album", type: "text" },
  { key: "year", type: "text" },
  { key: "comment", type: "text" },
];

export const OPS: OpDef[] = [
  // ---------------- Video ----------------
  {
    id: "video_convert", section: "video", category: "video", input: "files", formatParam: "format",
    fields: [
      { key: "preset", type: "select", options: "presets", optional: true, hintKey: "hints.preset" },
      { key: "format", type: "select", options: VIDEO_OUT, default: "mp4", showIf: (p) => !p.preset },
      { key: "codec", type: "select", options: ["h264", "hevc"], default: "h264",
        showIf: (p) => !p.preset && ["mp4", "mkv", "mov", "m4v", "ts"].includes(String(p.format)) },
      { key: "accel", type: "select", options: "accel", default: "auto", hintKey: "hints.accel" },
      { key: "crf", type: "number", min: 0, max: 51, step: 1, default: 23, hintKey: "hints.crf" },
      { key: "videoBitrateK", type: "number", min: 50, max: 200000, step: 50, hintKey: "hints.videoBitrate" },
      { key: "audioBitrateK", type: "number", min: 32, max: 512, step: 16 },
      { key: "maxWidth", type: "number", min: 16, max: 8192, step: 2 },
      { key: "maxHeight", type: "number", min: 16, max: 8192, step: 2 },
      { key: "targetSizeMb", type: "number", min: 1, max: 100000, step: 1, hintKey: "hints.targetSize" },
    ],
  },
  { id: "video_trim", section: "video", category: "video", input: "files", outputLabel: "same",
    fields: [
      { key: "start", type: "time", default: "0" },
      { key: "end", type: "time", hintKey: "hints.endOptional" },
      { key: "precise", type: "checkbox", default: false, hintKey: "hints.precise" },
    ] },
  { id: "video_split", section: "video", category: "video", input: "files", outputLabel: "folder",
    fields: [
      { key: "segmentSeconds", type: "number", min: 1, step: 1, default: 300 },
      { key: "precise", type: "checkbox", default: false, hintKey: "hints.precise" },
    ] },
  { id: "video_join", section: "video", category: "video", input: "files", combine: true, formatParam: "format",
    noticeKey: "notices.join", fields: [{ key: "format", type: "select", options: VIDEO_OUT.filter((f) => f !== "gif"), default: "mp4" }] },
  { id: "video_mux", section: "video", category: "video", input: "files", outputLabel: "same",
    fields: [{ key: "audio", type: "file" }] },
  { id: "video_crop", section: "video", category: "video", input: "files", outputLabel: "same", noticeKey: "notices.crop",
    fields: [
      { key: "x", type: "number", min: 0, default: 0 },
      { key: "y", type: "number", min: 0, default: 0 },
      { key: "w", type: "number", min: 2, default: 640 },
      { key: "h", type: "number", min: 2, default: 360 },
    ] },
  { id: "video_delogo", section: "video", category: "video", input: "files", outputLabel: "same",
    confirmParam: "authorized", noticeKey: "notices.delogo",
    fields: [
      { key: "x", type: "number", min: 1, default: 10 },
      { key: "y", type: "number", min: 1, default: 10 },
      { key: "w", type: "number", min: 2, default: 120 },
      { key: "h", type: "number", min: 2, default: 60 },
      { key: "authorized", type: "checkbox", default: false },
    ] },
  { id: "video_repair", section: "video", category: "video", input: "files", outputLabel: "mkv", noticeKey: "notices.repair", fields: [] },
  { id: "media_tags", section: "video", category: "media", input: "files", outputLabel: "same", fields: tagFields },

  // ---------------- Audio ----------------
  { id: "audio_convert", section: "audio", category: "media", input: "files", formatParam: "format",
    fields: [
      { key: "format", type: "select", options: AUDIO_OUT, default: "mp3" },
      { key: "bitrateK", type: "select", options: ["64", "96", "128", "160", "192", "256", "320"], default: "192", showIf: lossy },
      { key: "sampleRate", type: "select", options: ["22050", "32000", "44100", "48000", "96000"], optional: true },
      { key: "channels", type: "select", options: ["1", "2"], optional: true },
    ] },
  { id: "audio_trim", section: "audio", category: "media", input: "files", outputLabel: "same",
    fields: [{ key: "start", type: "time", default: "0" }, { key: "end", type: "time", hintKey: "hints.endOptional" }] },
  { id: "audio_split", section: "audio", category: "media", input: "files", outputLabel: "folder",
    fields: [{ key: "segmentSeconds", type: "number", min: 1, step: 1, default: 600 }] },
  { id: "audio_join", section: "audio", category: "media", input: "files", combine: true, formatParam: "format",
    fields: [{ key: "format", type: "select", options: AUDIO_OUT, default: "mp3" }] },
  { id: "audio_mix", section: "audio", category: "media", input: "files", combine: true, formatParam: "format",
    noticeKey: "notices.mix", fields: [{ key: "format", type: "select", options: AUDIO_OUT, default: "mp3" }] },
  { id: "audio_repair", section: "audio", category: "media", input: "files", outputLabel: "same", noticeKey: "notices.repair", fields: [] },
  { id: "audio_tags", section: "audio", category: "media", input: "files", outputLabel: "same", fields: tagFields },

  // ---------------- Image ----------------
  { id: "image_convert", section: "image", category: "image", input: "files", formatParam: "format",
    fields: [
      { key: "format", type: "select", options: IMAGE_OUT, default: "png", hintKey: "hints.heic" },
      { key: "quality", type: "number", min: 1, max: 100, default: 90, showIf: (p) => ["jpg", "webp", "heic", "avif"].includes(String(p.format)) },
      { key: "width", type: "number", min: 1, max: 30000 },
      { key: "height", type: "number", min: 1, max: 30000 },
      { key: "percent", type: "number", min: 1, max: 1000, hintKey: "hints.percent" },
      { key: "rotate", type: "select", options: ["0", "90", "180", "270"], default: "0" },
      { key: "flipH", type: "checkbox", default: false },
      { key: "flipV", type: "checkbox", default: false },
      { key: "keepMetadata", type: "checkbox", default: false, hintKey: "hints.keepMetadata" },
    ] },
  { id: "image_tags", section: "image", category: "image", input: "files", outputLabel: "same",
    fields: [{ key: "title", type: "text" }, { key: "artist", type: "text" }, { key: "copyright", type: "text" }, { key: "comment", type: "text" }] },

  // ---------------- PDF ----------------
  { id: "pdf_merge", section: "pdf", category: "pdf", input: "files", combine: true, outputLabel: "pdf", fields: [] },
  { id: "pdf_convert", section: "pdf", category: "pdf", input: "files", formatParam: "target", noticeKey: "notices.pdf",
    fields: [{ key: "target", type: "select", options: ["txt", "docx", "doc", "xlsx", "xls", "html", "htm"], default: "txt" }] },
  { id: "pdf_images", section: "pdf", category: "pdf", input: "files", outputLabel: "folder",
    fields: [{ key: "asJpg", type: "checkbox", default: true }] },

  // ---------------- Disc ----------------
  { id: "dvd_rip", section: "disc", input: "folder", formatParam: "format", confirmParam: "authorized", noticeKey: "notices.disc",
    fields: [
      { key: "preset", type: "select", options: "presets", optional: true },
      { key: "format", type: "select", options: VIDEO_OUT.filter((f) => f !== "gif"), default: "mp4", showIf: (p) => !p.preset },
      { key: "authorized", type: "checkbox", default: false },
    ] },
  { id: "bluray_rip", section: "disc", input: "folder", formatParam: "format", confirmParam: "authorized", noticeKey: "notices.disc",
    fields: [
      { key: "preset", type: "select", options: "presets", optional: true },
      { key: "format", type: "select", options: ["mp4", "mkv", "mov"], default: "mkv", showIf: (p) => !p.preset },
      { key: "authorized", type: "checkbox", default: false },
    ] },
  { id: "cd_rip", section: "disc", input: "folder", formatParam: "format", noticeKey: "notices.cd",
    fields: [
      { key: "format", type: "select", options: AUDIO_OUT, default: "flac" },
      { key: "bitrateK", type: "select", options: ["128", "192", "256", "320"], default: "256", showIf: lossy },
    ] },

  // ---------------- Archive ----------------
  { id: "archive_extract", section: "archive", category: "archive", input: "files", outputLabel: "folder",
    noticeKey: "notices.archive", fields: [] },
];

/** Some UI operations map onto a shared backend operation. */
export function backendOp(id: string): string {
  return id === "audio_tags" ? "media_tags" : id;
}

export const SECTIONS: Section[] = ["video", "audio", "image", "pdf", "disc", "download", "recorder", "archive"];

export function opsFor(section: Section): OpDef[] {
  return OPS.filter((o) => o.section === section);
}

export function defaults(op: OpDef): Params {
  const p: Params = {};
  for (const f of op.fields) if (f.default !== undefined) p[f.key] = f.default;
  return p;
}

/** Convert form params into the backend operation payload. */
export function toOperation(op: OpDef, params: Params): { op: string; [k: string]: unknown } {
  const out: { op: string; [k: string]: unknown } = { op: backendOp(op.id) };
  for (const f of op.fields) {
    if (f.showIf && !f.showIf(params)) continue;
    const v = params[f.key];
    if (v === undefined || v === "") continue;
    if (f.type === "number") {
      const n = Number(v);
      if (!Number.isNaN(n)) out[f.key] = n;
    } else if (f.type === "select" && typeof v === "string" && /^\d+$/.test(v) && f.key !== "format") {
      out[f.key] = Number(v);
    } else {
      out[f.key] = v;
    }
  }
  // Fields required by the backend even when hidden.
  if (op.id === "video_convert" && params.preset) out.format = "mp4";
  if ((op.id === "dvd_rip" || op.id === "bluray_rip") && params.preset) out.format = "mp4";
  return out;
}
