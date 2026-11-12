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
  /** Full path of the image file. */
  path: string;
}

export interface ItemInfo {
  id: string;
  name: string;
  category: string;
  width: number;
  height: number;
  createdAt: string;
  /** Full path of the prepared reference photo. */
  reference: string;
  outputs: OutputInfo[];
}

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
