/** 远程模式选文件:临时挂一个隐藏 <input type=file multiple>,点开系统选择器,resolve 选中的 File。
 *  取消(未选)时 change 不触发,靠 window focus 兜底 resolve 空数组,避免 Promise 永挂。 */
export function pickFilesViaInput(): Promise<File[]> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.multiple = true;
    input.style.display = "none";
    let settled = false;
    const done = (files: File[]) => {
      if (settled) return;
      settled = true;
      window.removeEventListener("focus", onFocus);
      input.remove();
      resolve(files);
    };
    input.addEventListener("change", () => done(input.files ? Array.from(input.files) : []));
    // 用户点「取消」:多数浏览器不发 change,只在窗口重新获焦后能判定放弃。延迟要给足:
    // iOS/低端安卓的 change 可比 refocus 晚数百毫秒,300ms 会把真选中的照片静默吞掉。
    const onFocus = () => setTimeout(() => done([]), 1000);
    window.addEventListener("focus", onFocus, { once: true });
    document.body.appendChild(input);
    input.click();
  });
}
