import { invoke } from "@tauri-apps/api/core";

/** 本窗口用上的系统材质（后端 material.rs 的 window_material）：
 *  mica = Windows 11 Mica；glass = macOS 26+ Liquid Glass；vibrancy = 旧版 macOS 的侧栏材质。 */
export type WindowMaterial = "none" | "mica" | "glass" | "vibrancy";

const MATERIALS: readonly WindowMaterial[] = ["none", "mica", "glass", "vibrancy"];

/** 启动时问后端本窗口有没有系统材质，有就给根元素加 `mat-on`（styles.css 据此把窗口底
 *  --cc-l0 从实色换成半透明染色层，并让 html / body 透明）。
 *  问不到（非 Tauri 环境、旧后端没有这个命令）或回了不认识的值一律当没有：宁可实色，
 *  不可在不透明窗口上铺半透明底。 */
export async function bootWindowMaterial(): Promise<WindowMaterial> {
  let material: WindowMaterial = "none";
  try {
    const answer = await invoke<string>("window_material");
    if ((MATERIALS as readonly string[]).includes(answer)) material = answer as WindowMaterial;
  } catch {
    material = "none";
  }
  document.documentElement.classList.toggle("mat-on", material !== "none");
  return material;
}
