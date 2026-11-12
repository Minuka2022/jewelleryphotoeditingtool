// Shapes of the data exchanged with the Rust backend (see src-tauri/src).

export type Tone = 'bright' | 'neutral' | 'dark';
/** Whether results look photographed or like a flawless 3D CAD render. */
export type Look = 'photo' | 'render';
export type OutputStatus = 'none' | 'approved' | 'rejected';

export interface Category {
  id: string;
  label: string;
}

export interface StyleInfo {
  id: string;
  name: string;
  version: number;
  description: string;
  tone: Tone;
  look: Look;
  /** Category ids the style is for. Empty means every category. */
  categories: string[];
  prompt: string;
  /** Full path of the example picture, if there is one. */
  thumbnail: string | null;
}

export interface ModelInfo {
  id: string;
  label: string;
  note: string;
  /** US dollars per image, by image size. */
  prices: Record<string, number>;
}

export interface LibraryInfo {
  categories: Category[];
  styles: StyleInfo[];
  models: ModelInfo[];
  pricesCheckedOn: string;
  problems: string[];
}

export interface Settings {
  model: string;
  imageSize: string;
  aspectRatio: string | null;
  variants: number;
}

export interface SettingsInfo extends Settings {
  hasApiKey: boolean;
}

/** A crop in fractions (0 to 1) of the image's width and height. */
export interface Crop {
  x: number;
  y: number;
  width: number;
  height: number;
}

export type ExportFormat = 'jpeg' | 'png' | 'webp';
export type Resample = 'lanczos' | 'bicubic' | 'bilinear' | 'nearest';

export interface ExportOptions {
  format: ExportFormat;
  /** 1 to 100; JPEG only. */
  quality: number;
  /** Width in pixels, or null to keep the size. Height follows the proportions. */
  width: number | null;
  resample: Resample;
  /** Pixels per inch written into JPEG files. */
  dpi: number;
}

export interface ExportEstimate {
  width: number;
  height: number;
  /** Size after cropping, before resizing. */
  sourceWidth: number;
  sourceHeight: number;
  bytes: number;
}

export interface OutputInfo {
  id: string;
  runId: string;
  styleId: string;
  styleName: string;
  styleVersion: number;
  status: OutputStatus;
  createdAt: string;
  model: string;
  imageSize: string;
  costUsd: number | null;
  note: string | null;
  /** The kept part of the image; the file itself is never changed. */
  crop?: Crop | null;
  /** Full path of the image to show: a cropped copy when there is a crop. */
  path: string;
  /** Full path of the uncropped original, used by the crop tool. */
  originalPath: string;
}

export interface ItemInfo {
  id: string;
  name: string;
  category: string;
  width: number;
  height: number;
  createdAt: string;
  /** Full path of the primary reference photo. */
  reference: string;
  /** Every photo of this piece, the primary one first. */
  references: ReferenceInfo[];
  /** What the AI found when it studied the photos, or the user's corrections. */
  designNotes: string | null;
  outputs: OutputInfo[];
}

export interface ReferenceInfo {
  id: string;
  path: string;
  width: number;
  height: number;
}

export interface AddReferencesResult {
  item: ItemInfo;
  errors: string[];
}

/** The most photos one piece can have. */
export const MAX_REFERENCES = 6;

export interface AddItemsResult {
  items: ItemInfo[];
  errors: string[];
}

export interface ImportStyle {
  name: string;
  description: string;
  prompt: string;
  tone: Tone;
  look: Look;
  categories: string[];
  imagePath: string | null;
}

export interface ImportedStyle {
  styleId: string;
  library: LibraryInfo;
}

/** Advice only: nothing here stops a style from being saved. */
export interface PromptCheck {
  warnings: string[];
  itemWords: string[];
}

export interface GenerateArgs {
  itemId: string;
  styleIds: string[];
  variants: number;
  note: string | null;
}

export interface OutputRef {
  itemId: string;
  outputId: string;
}

export interface UpdateInfo {
  version: string;
  currentVersion: string;
  notes: string | null;
}

export interface Progress {
  itemId: string;
  runId: string;
  done: number;
  total: number;
  styleId: string;
  styleName: string;
  number: number;
  output: OutputInfo | null;
  error: string | null;
}
