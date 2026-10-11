// 字节数的可读化。设置页「存储」、卸载弹窗、下载进度都用它，
// 原先这份逻辑长在 VbCableInstallDialog 内部，别处要显示体积就会复制第二份。

/** 把字节数格式化成 `1.5 GB` 这类人话。单位按 1024 进位，保留 1 位小数并去掉多余的 `.0` */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(k)), sizes.length - 1);
  return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + " " + sizes[i];
}
