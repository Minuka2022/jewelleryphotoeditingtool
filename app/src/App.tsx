import { useEffect, useRef } from 'react';
import { api, inDesktopApp } from './api';
import { Canvas } from './components/Canvas';
import { MenuBar, OptionsBar, StatusBar, Tabs, ToolStrip, UpdateBanner } from './components/Chrome';
import { Dialogs } from './components/Dialogs';
import { Filmstrip, SidePanels } from './components/Panels';
import { FIT, StudioProvider, useStudio } from './store';

const PHOTO_FILE = /\.(jpe?g|png|webp)$/i;

/** Keyboard shortcuts and photos dropped onto the window. */
function useGlobalInput() {
  const studio = useStudio();
  // The listeners are attached once; this keeps them pointed at current state.
  const latest = useRef(studio);
  latest.current = studio;

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const studio = latest.current;
      if (studio.dialog || studio.confirmation) return;
      const key = event.key.toLowerCase();
      const control = event.ctrlKey || event.metaKey;
      const run = (action: () => void) => {
        event.preventDefault();
        action();
      };

      // While cropping, Enter applies and Esc cancels; other shortcuts wait.
      if (studio.cropDraft) {
        if (key === 'enter') return run(studio.applyCrop);
        if (key === 'escape') return run(studio.cancelCrop);
        return;
      }

      if (control && key === 'o') return run(studio.openPhotos);
      if (control && event.shiftKey && key === 'e') return run(studio.openExport);
      if (control && (key === 'g' || key === 'enter')) return run(studio.generate);

      const typing = ['INPUT', 'TEXTAREA', 'SELECT'].includes((event.target as HTMLElement).tagName);
      if (typing || control || event.altKey) return;
      const actions: Record<string, () => void> = {
        a: () => studio.setStatus('approved'),
        x: () => studio.setStatus('rejected'),
        delete: studio.deleteViewedOutput,
        arrowright: () => studio.stepOutput(1),
        arrowleft: () => studio.stepOutput(-1),
        1: () => studio.setCompare('single'),
        2: () => studio.setCompare('split'),
        3: () => studio.setCompare('slider'),
        0: () => studio.setView(FIT),
        h: () => studio.setTool('hand'),
        z: () => studio.setTool('zoom'),
        c: () => studio.setTool('crop'),
      };
      if (actions[key]) run(actions[key]);
    };

    // The built-in browser menu (Back, Reload, Inspect) has no place in the app.
    const onContextMenu = (event: MouseEvent) => inDesktopApp && event.preventDefault();

    window.addEventListener('keydown', onKey);
    window.addEventListener('contextmenu', onContextMenu);
    const stopDrops = api.onFilesDropped(
      (paths) => void latest.current.addPhotos(paths.filter((path) => PHOTO_FILE.test(path))),
      (message) => latest.current.notify('error', message),
    );
    return () => {
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('contextmenu', onContextMenu);
      void stopDrops.then((stop) => stop());
    };
  }, []);
}

function Workspace() {
  const { startupError } = useStudio();
  useGlobalInput();

  if (startupError) {
    return (
      <div className="flex h-full items-center justify-center p-10">
        <div className="max-w-lg space-y-2 text-center">
          <p className="text-[15px] font-semibold">Jewelry Studio could not start</p>
          <p className="text-dim select-text">{startupError}</p>
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      <MenuBar />
      <OptionsBar />
      <UpdateBanner />
      <div className="flex min-h-0 flex-1">
        <ToolStrip />
        <main className="flex min-w-0 flex-1 flex-col">
          <Tabs />
          <Canvas />
          <Filmstrip />
        </main>
        <SidePanels />
      </div>
      <StatusBar />
      <Dialogs />
    </div>
  );
}

export default function App() {
  return (
    <StudioProvider>
      <Workspace />
    </StudioProvider>
  );
}
