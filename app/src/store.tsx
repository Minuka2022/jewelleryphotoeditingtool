// All of the interface's state and the actions that change it.

import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { api, errorText } from './api';
import type {
  ImportStyle,
  ItemInfo,
  LibraryInfo,
  OutputInfo,
  OutputStatus,
  Settings,
  SettingsInfo,
  StyleInfo,
  UpdateInfo,
} from './types';

export type CompareMode = 'single' | 'split' | 'slider';
export type Tool = 'hand' | 'zoom';
export type DialogName = 'settings' | 'import-style' | 'shortcuts';

/** Zoom 1 means "fit the window"; x and y are the pan offset in pixels. */
export interface View {
  zoom: number;
  x: number;
  y: number;
}

export interface Toast {
  id: number;
  kind: 'info' | 'error';
  text: string;
}

export interface Confirmation {
  title: string;
  body: string;
  action: string;
  danger?: boolean;
  run: () => void;
}

export const FIT: View = { zoom: 1, x: 0, y: 0 };

/** Batches that cost at least this much ask for confirmation first. */
const CONFIRM_ABOVE_USD = 1;

export function formatUsd(amount: number): string {
  return amount < 10 ? `$${amount.toFixed(2)}` : `$${amount.toFixed(0)}`;
}

export function styleFits(style: StyleInfo, category: string): boolean {
  return style.categories.length === 0 || style.categories.includes(category);
}

function useController() {
  const [library, setLibrary] = useState<LibraryInfo | null>(null);
  const [settings, setSettings] = useState<SettingsInfo | null>(null);
  const [items, setItems] = useState<ItemInfo[]>([]);
  const [startupError, setStartupError] = useState<string | null>(null);

  const [activeId, setActiveId] = useState<string | null>(null);
  // Per photo: the generated image being viewed, or null for the reference.
  const [viewing, setViewing] = useState<Record<string, string | null>>({});
  const [selectedStyles, setSelectedStyles] = useState<string[]>([]);
  // Which styles the panel lists: "auto" follows the open photo's type,
  // "all" lists everything, anything else is a jewelry type id.
  const [styleFilter, setStyleFilter] = useState('auto');
  // The style being edited in the style window, or null when adding a new one.
  const [editingStyle, setEditingStyle] = useState<StyleInfo | null>(null);
  const [note, setNote] = useState('');
  const [lastCategory, setLastCategory] = useState('jewelry');

  const [compare, setCompare] = useState<CompareMode>('split');
  const [tool, setTool] = useState<Tool>('hand');
  const [view, setView] = useState<View>(FIT);
  const [zoomPercent, setZoomPercent] = useState(100);

  // Per photo that is generating: how many images are done out of how many.
  const [busy, setBusy] = useState<Record<string, { done: number; total: number }>>({});
  const [dialog, setDialog] = useState<DialogName | null>(null);
  const [confirmation, setConfirmation] = useState<Confirmation | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const toastId = useRef(0);
  const runErrors = useRef<Record<string, string[]>>({});

  const [version, setVersion] = useState('');
  // A newer version that is ready to install, if there is one.
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [updating, setUpdating] = useState(false);

  const dismissToast = useCallback((id: number) => {
    setToasts((list) => list.filter((toast) => toast.id !== id));
  }, []);

  const notify = useCallback(
    (kind: Toast['kind'], text: string) => {
      const id = ++toastId.current;
      setToasts((list) => [...list.slice(-3), { id, kind, text }]);
      setTimeout(() => dismissToast(id), kind === 'error' ? 15000 : 5000);
    },
    [dismissToast],
  );

  const fail = useCallback((error: unknown) => notify('error', errorText(error)), [notify]);

  const putItem = useCallback((item: ItemInfo) => {
    setItems((list) => list.map((existing) => (existing.id === item.id ? item : existing)));
  }, []);

  // Load everything once, and listen for images arriving from a generation.
  useEffect(() => {
    Promise.all([api.getLibrary(), api.getSettings(), api.listItems()])
      .then(([loadedLibrary, loadedSettings, loadedItems]) => {
        setLibrary(loadedLibrary);
        setSettings(loadedSettings);
        setItems(loadedItems);
        setActiveId(loadedItems[0]?.id ?? null);
        loadedLibrary.problems.forEach((problem) => notify('error', `A style was skipped. ${problem}`));
      })
      .catch((error) => setStartupError(errorText(error)));

    api.getVersion().then(setVersion, () => {});
    // A failed check at startup (no internet, for example) is not worth a message.
    api.checkForUpdate().then(setUpdate, () => {});

    const stopListening = api.onProgress((progress) => {
      setBusy((current) =>
        current[progress.itemId]
          ? { ...current, [progress.itemId]: { done: progress.done, total: progress.total } }
          : current,
      );
      if (progress.error) {
        (runErrors.current[progress.itemId] ??= []).push(progress.error);
      }
      const output = progress.output;
      if (!output) return;
      setItems((list) =>
        list.map((item) =>
          item.id === progress.itemId && !item.outputs.some((o) => o.id === output.id)
            ? { ...item, outputs: [...item.outputs, output] }
            : item,
        ),
      );
      // Show the first result as soon as it arrives.
      setViewing((current) => (current[progress.itemId] ? current : { ...current, [progress.itemId]: output.id }));
    });
    return () => void stopListening.then((stop) => stop());
  }, [notify]);

  const activeItem = useMemo(() => items.find((item) => item.id === activeId) ?? null, [items, activeId]);
  const viewedOutput: OutputInfo | null = useMemo(() => {
    if (!activeItem) return null;
    return activeItem.outputs.find((output) => output.id === viewing[activeItem.id]) ?? null;
  }, [activeItem, viewing]);

  // The selected styles that can be used for the active photo's category.
  const usableStyles = useMemo(() => {
    if (!library || !activeItem) return [];
    return library.styles.filter(
      (style) => selectedStyles.includes(style.id) && styleFits(style, activeItem.category),
    );
  }, [library, activeItem, selectedStyles]);

  const pricePerImage = useMemo(() => {
    if (!library || !settings) return null;
    return library.models.find((model) => model.id === settings.model)?.prices[settings.imageSize] ?? null;
  }, [library, settings]);

  const plannedImages = usableStyles.length * (settings?.variants ?? 0);
  const plannedCost = pricePerImage === null ? null : pricePerImage * plannedImages;
  const isGenerating = activeItem ? Boolean(busy[activeItem.id]) : false;

  const selectItem = useCallback((id: string) => {
    setActiveId(id);
    setView(FIT);
  }, []);

  const viewOutput = useCallback((itemId: string, outputId: string | null) => {
    setViewing((current) => ({ ...current, [itemId]: outputId }));
  }, []);

  /** Moves to the next or previous image in the filmstrip (the reference comes first). */
  const stepOutput = useCallback(
    (direction: 1 | -1) => {
      if (!activeItem) return;
      const order: (string | null)[] = [null, ...activeItem.outputs.map((output) => output.id)];
      const position = order.indexOf(viewedOutput?.id ?? null);
      const next = order[(position + direction + order.length) % order.length];
      viewOutput(activeItem.id, next);
    },
    [activeItem, viewedOutput, viewOutput],
  );

  const addPhotos = useCallback(
    async (paths: string[]) => {
      if (paths.length === 0) return;
      try {
        const result = await api.addItems(paths, lastCategory);
        setItems((list) => [...list, ...result.items]);
        if (result.items.length > 0) selectItem(result.items[0].id);
        result.errors.forEach((error) => notify('error', error));
      } catch (error) {
        fail(error);
      }
    },
    [lastCategory, selectItem, notify, fail],
  );

  const openPhotos = useCallback(async () => {
    try {
      await addPhotos(await api.pickPhotos());
    } catch (error) {
      fail(error);
    }
  }, [addPhotos, fail]);

  const updateItem = useCallback(
    async (id: string, patch: { name?: string; category?: string }) => {
      try {
        putItem(await api.updateItem(id, patch));
        if (patch.category) setLastCategory(patch.category);
      } catch (error) {
        fail(error);
      }
    },
    [putItem, fail],
  );

  const removeItem = useCallback(
    (item: ItemInfo) => {
      setConfirmation({
        title: `Remove "${item.name}"?`,
        body:
          item.outputs.length > 0
            ? `This deletes the photo and its ${item.outputs.length} generated image(s) from Jewelry Studio. Files you already exported are not affected.`
            : 'This removes the photo from Jewelry Studio. Your original file is not affected.',
        action: 'Remove',
        danger: true,
        run: async () => {
          try {
            await api.removeItem(item.id);
            const next = items.find((existing) => existing.id !== item.id)?.id ?? null;
            setItems((list) => list.filter((existing) => existing.id !== item.id));
            setActiveId((current) => (current === item.id ? next : current));
          } catch (error) {
            fail(error);
          }
        },
      });
    },
    [items, fail],
  );

  const setStatus = useCallback(
    async (status: OutputStatus) => {
      if (!activeItem || !viewedOutput) return;
      // Pressing the same key again clears the mark.
      const next = viewedOutput.status === status ? 'none' : status;
      try {
        putItem(await api.setOutputStatus(activeItem.id, viewedOutput.id, next));
      } catch (error) {
        fail(error);
      }
    },
    [activeItem, viewedOutput, putItem, fail],
  );

  const deleteViewedOutput = useCallback(async () => {
    if (!activeItem || !viewedOutput) return;
    try {
      putItem(await api.removeOutput(activeItem.id, viewedOutput.id));
      viewOutput(activeItem.id, null);
    } catch (error) {
      fail(error);
    }
  }, [activeItem, viewedOutput, putItem, viewOutput, fail]);

  const toggleStyle = useCallback((id: string) => {
    setSelectedStyles((list) => (list.includes(id) ? list.filter((other) => other !== id) : [...list, id]));
  }, []);

  const changeSettings = useCallback(
    async (patch: Partial<Settings>) => {
      if (!settings) return;
      const { hasApiKey: _unused, ...current } = settings;
      try {
        setSettings(await api.saveSettings({ ...current, ...patch }));
      } catch (error) {
        fail(error);
      }
    },
    [settings, fail],
  );

  const runGeneration = useCallback(async () => {
    if (!activeItem || !settings || usableStyles.length === 0) return;
    const itemId = activeItem.id;
    const total = usableStyles.length * settings.variants;
    runErrors.current[itemId] = [];
    setBusy((current) => ({ ...current, [itemId]: { done: 0, total } }));
    try {
      putItem(
        await api.generate({
          itemId,
          styleIds: usableStyles.map((style) => style.id),
          variants: settings.variants,
          note: note.trim() || null,
        }),
      );
      const errors = runErrors.current[itemId] ?? [];
      if (errors.length > 0) {
        notify('error', `${errors.length} of ${total} images could not be generated. ${errors[0]}`);
      }
    } catch (error) {
      fail(error);
    } finally {
      setBusy(({ [itemId]: _finished, ...rest }) => rest);
    }
  }, [activeItem, settings, usableStyles, note, putItem, notify, fail]);

  const generate = useCallback(() => {
    if (!activeItem || isGenerating) return;
    if (!settings?.hasApiKey) {
      setDialog('settings');
      return;
    }
    if (usableStyles.length === 0) {
      notify('info', 'Choose at least one style in the Styles panel first.');
      return;
    }
    if (plannedCost !== null && plannedCost >= CONFIRM_ABOVE_USD) {
      setConfirmation({
        title: `Generate ${plannedImages} images?`,
        body: `This will cost about ${formatUsd(plannedCost)}.`,
        action: 'Generate',
        run: runGeneration,
      });
      return;
    }
    void runGeneration();
  }, [activeItem, isGenerating, settings, usableStyles, plannedCost, plannedImages, notify, runGeneration]);

  const exportApproved = useCallback(async () => {
    const selections = items.flatMap((item) =>
      item.outputs
        .filter((output) => output.status === 'approved')
        .map((output) => ({ itemId: item.id, outputId: output.id })),
    );
    if (selections.length === 0) {
      notify('info', 'Nothing to export yet. Approve the images you want to keep (press A), then export.');
      return;
    }
    try {
      const destination = await api.pickFolder();
      if (!destination) return;
      const copied = await api.exportOutputs(selections, destination);
      notify('info', `Exported ${copied} image(s) to ${destination}`);
    } catch (error) {
      fail(error);
    }
  }, [items, notify, fail]);

  /** Opens the style window: empty to add a style, or filled in to edit one. */
  const openStyleDialog = useCallback((style?: StyleInfo) => {
    setEditingStyle(style ?? null);
    setDialog('import-style');
  }, []);

  /** Saves the style window. Returns a message if the style was refused. */
  const saveStyle = useCallback(
    async (style: ImportStyle): Promise<string | null> => {
      try {
        if (editingStyle) {
          const result = await api.updateStyle(editingStyle.id, style);
          setLibrary(result.library);
          notify('info', `"${style.name.trim()}" was updated.`);
        } else {
          const result = await api.importStyle(style);
          setLibrary(result.library);
          setSelectedStyles((list) => [...list, result.styleId]);
          notify('info', `"${style.name.trim()}" was added to your styles.`);
        }
        return null;
      } catch (error) {
        return errorText(error);
      }
    },
    [editingStyle, notify],
  );

  const removeStyle = useCallback(
    (style: StyleInfo) => {
      setConfirmation({
        title: `Delete the style "${style.name}"?`,
        body: 'Images already generated with it are kept.',
        action: 'Delete',
        danger: true,
        run: async () => {
          try {
            setLibrary(await api.removeStyle(style.id));
            setSelectedStyles((list) => list.filter((id) => id !== style.id));
          } catch (error) {
            fail(error);
          }
        },
      });
    },
    [fail],
  );

  const checkForUpdates = useCallback(async () => {
    try {
      const found = await api.checkForUpdate();
      setUpdate(found);
      if (!found) notify('info', `You have the latest version${version ? ` (${version})` : ''}.`);
    } catch (error) {
      fail(error);
    }
  }, [version, notify, fail]);

  /** On success the app closes, installs and restarts, so this never returns. */
  const installUpdate = useCallback(async () => {
    setUpdating(true);
    try {
      await api.installUpdate();
    } catch (error) {
      fail(error);
      setUpdating(false);
    }
  }, [fail]);

  return {
    version,
    update,
    setUpdate,
    updating,
    checkForUpdates,
    installUpdate,
    library,
    settings,
    setSettings,
    items,
    startupError,
    activeItem,
    viewedOutput,
    selectedStyles,
    usableStyles,
    note,
    setNote,
    compare,
    setCompare,
    tool,
    setTool,
    view,
    setView,
    zoomPercent,
    setZoomPercent,
    busy,
    isGenerating,
    plannedImages,
    plannedCost,
    dialog,
    setDialog,
    confirmation,
    setConfirmation,
    toasts,
    dismissToast,
    notify,
    selectItem,
    viewOutput,
    stepOutput,
    addPhotos,
    openPhotos,
    updateItem,
    removeItem,
    setStatus,
    deleteViewedOutput,
    toggleStyle,
    changeSettings,
    generate,
    exportApproved,
    styleFilter,
    setStyleFilter,
    editingStyle,
    openStyleDialog,
    saveStyle,
    removeStyle,
  };
}

export type Studio = ReturnType<typeof useController>;

const StudioContext = createContext<Studio | null>(null);

export function StudioProvider({ children }: { children: ReactNode }) {
  return <StudioContext.Provider value={useController()}>{children}</StudioContext.Provider>;
}

export function useStudio(): Studio {
  const studio = useContext(StudioContext);
  if (!studio) throw new Error('useStudio must be used inside StudioProvider');
  return studio;
}
