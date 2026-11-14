/**
 * 附件标注解析与展示工具（ADR-0015）。
 *
 * 服务端档案的 user 行存「原文 + 每附件一行标注」（QQ annotated_text 同款，
 * robit-agent `MediaAttachment::describe_with_local` 输出）：
 *   `[用户发送了文件: 对账单.csv (50.0KB), 本地路径: media/对账单.csv]`
 *   `[用户发送了图片: 截图.png (12.3KB), 本地路径: media/截图.png]`
 * 本模块把标注行从正文剥离、还原成附件 chips 元数据——live 发送（File 元数据）
 * 与档案水合（标注解析）共用同一展示形态。纯函数，vitest node 环境直测。
 *
 * 解析对 robit 标注格式宽容：size/path 段可选，文件名含空格/括号不误伤
 * （size 后缀锚定行尾、path 取 lastIndexOf）；无法识别的行一律保留进正文，
 * 不静默丢字（与全站「降级如实」纪律一致）。
 */

/** 附件展示元数据（size 单位字节；标注解析自 KB 换算，live 取自 File.size） */
export interface FileMeta {
  name: string;
  size: number;
}

/** 客户端预检上限（镜像服务端缺省；服务端 [app.context] max_media_bytes 是最终裁判，超限 413） */
export const MAX_FILE_BYTES = 20 * 1024 * 1024;
/** 单条消息附件数量上限（镜像宿主 DEFAULT_MAX_FILES） */
export const MAX_FILES = 10;

const ANNOTATION_RE = /^\[用户发送了(?:图片|文件): (.*)\]$/;
const PATH_SEP = ", 本地路径: ";
const SIZE_RE = / \(([\d.]+)KB\)$/;

/**
 * 剥离 user 文本里的附件标注行 → { body（正文）, files（附件元数据） }。
 * 无标注时 body 原文返回、files 空。
 */
export function parseFileAnnotations(text: string): { body: string; files: FileMeta[] } {
  const bodyLines: string[] = [];
  const files: FileMeta[] = [];
  for (const line of text.split("\n")) {
    const m = line.match(ANNOTATION_RE);
    if (!m) {
      bodyLines.push(line);
      continue;
    }
    let inner = m[1];
    // 先剥本地路径段（取最后一个分隔符，文件名里可能含逗号）
    const pathIdx = inner.lastIndexOf(PATH_SEP);
    if (pathIdx >= 0) inner = inner.slice(0, pathIdx);
    // 再剥尺寸段（锚定行尾）
    let size = 0;
    const sizeM = inner.match(SIZE_RE);
    if (sizeM && sizeM.index !== undefined) {
      size = Math.round(parseFloat(sizeM[1]) * 1024);
      inner = inner.slice(0, sizeM.index);
    }
    const name = inner.trim();
    if (!name) {
      // 名字解析不出 → 整行保留进正文，不丢
      bodyLines.push(line);
      continue;
    }
    files.push({ name, size });
  }
  return { body: bodyLines.join("\n"), files };
}

/** 字节数 → 人类可读尺寸（0/负 = 未知，返回空串） */
export function formatFileSize(bytes: number): string {
  if (!bytes || bytes <= 0) return "";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
