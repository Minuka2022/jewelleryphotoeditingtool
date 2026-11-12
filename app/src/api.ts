// The one place the interface talks to the Rust backend.

import { getVersion } from '@tauri-apps/api/app';
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { open } from '@tauri-apps/plugin-dialog';
import { mockApi } from './mock';
import type {
  AddItemsResult,
  GenerateArgs,
  ImportStyle,
  ImportedStyle,
  ItemInfo,
  LibraryInfo,
  OutputRef,
  OutputStatus,
  Progress,
  PromptCheck,
  Settings,
  SettingsInfo,
  UpdateInfo,
} from './types';

export interface Api {
  getLibrary(): Promise<LibraryInfo>;
  getSettings(): Promise<SettingsInfo>;
  saveSettings(settings: Settings): Promise<SettingsInfo>;
  /** A blank key removes the saved key. */
  setApiKey(key: string): Promise<SettingsInfo>;
  checkApiKey(): Promise<string>;
  listItems(): Promise<ItemInfo[]>;
  addItems(paths: string[], category: string): Promise<AddItemsResult>;
  updateItem(id: string, patch: { name?: string; category?: string }): Promise<ItemInfo>;
  removeItem(id: string): Promise<void>;
  setOutputStatus(itemId: string, outputId: string, status: OutputStatus): Promise<ItemInfo>;
  removeOutput(itemId: string, outputId: string): Promise<ItemInfo>;
  exportOutputs(selections: OutputRef[], destination: string): Promise<number>;
  checkStylePrompt(prompt: string): Promise<PromptCheck>;
  importStyle(style: ImportStyle): Promise<ImportedStyle>;
  /** A null `imagePath` keeps the style's current picture. */
  updateStyle(id: string, style: ImportStyle): Promise<ImportedStyle>;
  removeStyle(id: string): Promise<LibraryInfo>;
  generate(args: GenerateArgs): Promise<ItemInfo>;
  getVersion(): Promise<string>;
  /** Resolves to null when this is the newest version. */
  checkForUpdate(): Promise<UpdateInfo | null>;
  /** Downloads and installs the newer version, then restarts the app. */
  installUpdate(): Promise<void>;
  /** Both return a function that stops listening. */
  onProgress(handler: (progress: Progress) => void): Promise<() => void>;
  onFilesDropped(handler: (paths: string[]) => void): Promise<() => void>;
  pickPhotos(): Promise<string[]>;
  pickImage(): Promise<string | null>;
  pickFolder(): Promise<string | null>;
  /** Turns a file path from the backend into something an <img> can show. */
  fileSrc(path: string): string;
}

const PHOTO_FILTER = [{ name: 'Photos', extensions: ['jpg', 'jpeg', 'png', 'webp'] }];

const tauriApi: Api = {
  getLibrary: () => invoke('get_library'),
  getSettings: () => invoke('get_settings'),
  saveSettings: (settings) => invoke('save_settings', { settings }),
  setApiKey: (key) => invoke('set_api_key', { key }),
  checkApiKey: () => invoke('check_api_key'),
  listItems: () => invoke('list_items'),
  addItems: (paths, category) => invoke('add_items', { paths, category }),
  updateItem: (id, patch) => invoke('update_item', { id, ...patch }),
  removeItem: (id) => invoke('remove_item', { id }),
  setOutputStatus: (itemId, outputId, status) =>
    invoke('set_output_status', { itemId, outputId, status }),
  removeOutput: (itemId, outputId) => invoke('remove_output', { itemId, outputId }),
  exportOutputs: (selections, destination) => invoke('export_outputs', { selections, destination }),
  checkStylePrompt: (prompt) => invoke('check_style_prompt', { prompt }),
  importStyle: (style) => invoke('import_style', { style }),
  updateStyle: (id, style) => invoke('update_style', { id, style }),
  removeStyle: (id) => invoke('remove_style', { id }),
  generate: (args) => invoke('generate', { args }),
  getVersion,
  checkForUpdate: () => invoke('check_for_update'),
  installUpdate: () => invoke('install_update'),
  onProgress: (handler) => listen<Progress>('generation-progress', (event) => handler(event.payload)),
  onFilesDropped: (handler) =>
    getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === 'drop') handler(event.payload.paths);
    }),
  pickPhotos: async () => (await open({ multiple: true, filters: PHOTO_FILTER })) ?? [],
  pickImage: () => open({ multiple: false, filters: PHOTO_FILTER }),
  pickFolder: () => open({ directory: true }),
  fileSrc: (path) => convertFileSrc(path),
};

export const inDesktopApp = '__TAURI_INTERNALS__' in window;

export const api: Api = inDesktopApp ? tauriApi : mockApi;

/** The backend reports problems as plain sentences meant for the user. */
export function errorText(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  return 'Something went wrong.';
}
