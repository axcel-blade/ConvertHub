// Typed wrappers around the Rust commands.
import { invoke } from "@tauri-apps/api/core";
import type { UiMsg } from "./i18n";

export type Status = "queued" | "running" | "done" | "failed" | "cancelled";
export type Operation = { op: string; [k: string]: unknown };

export interface JobRequest {
  inputs: string[];
  outputDir: string;
  outputName?: string;
  overwrite: boolean;
  operation: Operation;
}

export interface Job {
  id: string;
  request: JobRequest;
  status: Status;
  progress: number;
  message: UiMsg | null;
  outputs: string[];
  engine: string | null;
  createdAt: number;
  finishedAt: number | null;
}

export interface DepStatus {
  tool: string;
  found: boolean;
  path: string | null;
  version: string | null;
}

export interface SystemInfo {
  os: string;
  arch: string;
  version: string;
  deps: DepStatus[];
}

export interface HwInfo {
  ffmpegFound: boolean;
  hwaccels: string[];
  compiled: string[];
  verified: { name: string; codec: string; vendor: string }[];
}

export interface InputCheck {
  path: string;
  ok: boolean;
  size: number;
  error: UiMsg | null;
}

export interface Preset {
  id: string;
  group: string;
  format: string;
  codec: string;
  maxWidth: number;
  maxHeight: number;
}

export interface Source {
  id: string;
  name: string;
  hosts: string[];
  enabled: boolean;
  noteKey: string;
}

export interface Probe {
  duration: number | null;
  hasVideo: boolean;
  hasAudio: boolean;
  width: number | null;
  height: number | null;
  videoCodec: string | null;
  audioCodec: string | null;
}

export interface CaptureTargets {
  supported: boolean;
  reasonKey: string | null;
  windowCapture: boolean;
  displays: { id: string; name: string; width: number; height: number }[];
  windows: { id: string; title: string }[];
}

export const api = {
  systemInfo: () => invoke<SystemInfo>("system_info"),
  hardwareInfo: (refresh = false) => invoke<HwInfo>("hardware_info", { refresh }),
  validateInputs: (paths: string[], category: string) => invoke<InputCheck[]>("validate_inputs", { paths, category }),
  checkConflicts: (requests: JobRequest[]) => invoke<string[]>("check_conflicts", { requests }),
  submitJobs: (requests: JobRequest[]) => invoke<string[]>("submit_jobs", { requests }),
  startQueue: () => invoke<void>("start_queue"),
  pauseQueue: () => invoke<void>("pause_queue"),
  queueState: () => invoke<{ running: boolean }>("queue_state"),
  listJobs: () => invoke<Job[]>("list_jobs"),
  cancelJob: (id: string) => invoke<void>("cancel_job", { id }),
  retryJob: (id: string) => invoke<string | null>("retry_job", { id }),
  removeJob: (id: string) => invoke<void>("remove_job", { id }),
  clearFinished: () => invoke<void>("clear_finished"),
  recentJobs: () => invoke<Job[]>("recent_jobs"),
  clearRecent: () => invoke<void>("clear_recent"),
  probeMedia: (path: string) => invoke<Probe>("probe_media", { path }),
  imageInfo: (path: string) =>
    invoke<{ width: number; height: number; format: string; tags: [string, string][] }>("image_info", { path }),
  presets: () => invoke<Preset[]>("list_presets"),
  downloadSources: () => invoke<Source[]>("download_sources"),
  formatGuide: () => invoke<Record<string, string[]>>("format_guide"),
  captureTargets: () => invoke<CaptureTargets>("capture_targets"),
  startRecording: (options: Record<string, unknown>) => invoke<string>("start_recording", { options }),
  stopRecording: () => invoke<string>("stop_recording"),
  recordingState: () => invoke<{ recording: boolean; elapsedSecs: number }>("recording_state"),
  defaultOutputDir: () => invoke<string | null>("default_output_dir"),
  pathExists: (path: string) => invoke<boolean>("path_exists", { path }),
};
