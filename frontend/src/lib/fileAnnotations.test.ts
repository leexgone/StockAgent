import { describe, expect, it } from "vitest";
import { formatFileSize, parseFileAnnotations } from "./fileAnnotations";

describe("parseFileAnnotations", () => {
  it("无标注 → 原文返回、files 空", () => {
    expect(parseFileAnnotations("今天该做什么")).toEqual({ body: "今天该做什么", files: [] });
    expect(parseFileAnnotations("")).toEqual({ body: "", files: [] });
  });

  it("单文件标注（带尺寸与路径）→ 剥离正文、还原元数据", () => {
    const text = "解析这个\n[用户发送了文件: 对账单.csv (50.0KB), 本地路径: media/对账单.csv]";
    expect(parseFileAnnotations(text)).toEqual({
      body: "解析这个",
      files: [{ name: "对账单.csv", size: 50 * 1024 }],
    });
  });

  it("图片标注同样识别", () => {
    const text = "[用户发送了图片: 截图.png (12.3KB), 本地路径: media/截图.png]";
    const { body, files } = parseFileAnnotations(text);
    expect(body).toBe("");
    expect(files).toEqual([{ name: "截图.png", size: Math.round(12.3 * 1024) }]);
  });

  it("多附件 + 正文交错", () => {
    const text = [
      "看下这两份",
      "[用户发送了文件: a.csv (1.0KB), 本地路径: media/a.csv]",
      "[用户发送了文件: b.csv (2.0KB), 本地路径: media/b-1.csv]",
    ].join("\n");
    expect(parseFileAnnotations(text)).toEqual({
      body: "看下这两份",
      files: [
        { name: "a.csv", size: 1024 },
        { name: "b.csv", size: 2048 },
      ],
    });
  });

  it("尺寸段可选", () => {
    const text = "[用户发送了文件: note.txt, 本地路径: media/note.txt]";
    expect(parseFileAnnotations(text).files).toEqual([{ name: "note.txt", size: 0 }]);
  });

  it("路径段可选", () => {
    const text = "[用户发送了文件: note.txt (3.5KB)]";
    expect(parseFileAnnotations(text).files).toEqual([{ name: "note.txt", size: Math.round(3.5 * 1024) }]);
  });

  it("文件名含空格/括号/逗号不误伤", () => {
    const text = "[用户发送了文件: 报表 (终稿), v2.csv (8.0KB), 本地路径: media/报表 (终稿), v2.csv]";
    expect(parseFileAnnotations(text).files).toEqual([{ name: "报表 (终稿), v2.csv", size: 8 * 1024 }]);
  });

  it("无法识别的标注行保留进正文（不静默丢字）", () => {
    const text = "[用户发送了文件: ]";
    expect(parseFileAnnotations(text)).toEqual({ body: "[用户发送了文件: ]", files: [] });
  });

  it("普通正文里的方括号不被误判", () => {
    const text = "参考 [文档] 说明";
    expect(parseFileAnnotations(text)).toEqual({ body: "参考 [文档] 说明", files: [] });
  });
});

describe("formatFileSize", () => {
  it("0/负 = 未知 → 空串", () => {
    expect(formatFileSize(0)).toBe("");
    expect(formatFileSize(-1)).toBe("");
  });
  it("字节/KB/MB 分档", () => {
    expect(formatFileSize(512)).toBe("512 B");
    expect(formatFileSize(50 * 1024)).toBe("50.0 KB");
    expect(formatFileSize(3 * 1024 * 1024)).toBe("3.0 MB");
  });
});
