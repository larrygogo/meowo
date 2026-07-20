import { useEffect, useRef } from "react";
import { listen, type Event } from "@tauri-apps/api/event";

/**
 * Subscribe once per event name while always invoking the latest render's handler.
 *
 * Tauri resolves `listen` asynchronously. The disposed branch closes the small race
 * where a component unmounts before registration finishes, which otherwise leaks a
 * listener for the rest of the window lifetime.
 */
export function useTauriEvent<T>(event: string, handler: (event: Event<T>) => void): void {
  const handlerRef = useRef(handler);
  handlerRef.current = handler;

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    void listen<T>(event, (incoming) => handlerRef.current(incoming))
      .then((dispose) => {
        if (disposed) dispose();
        else unlisten = dispose;
      })
      .catch(() => {});

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [event]);
}
