// 本机图片内联：路径提取纯函数矩阵 + MessageImages 组件三态（图/缺失/错误）。
import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { extractLocalImagePaths } from "../localImages";
import MessageImages, { clearImageCache } from "../MessageImages";
import { invoke } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => null),
}));

describe("extractLocalImagePaths 提取矩阵", () => {
  it("markdown 图片语法（含尖括号带空格路径）", () => {
    const r = extractLocalImagePaths(
      "截图如下 ![shot](/Users/tom/Pictures/shot.png) 与 ![带空格](</Users/tom/My Files/a b.png>)",
    );
    expect(r.map((x) => x.path)).toEqual([
      "/Users/tom/Pictures/shot.png",
      "/Users/tom/My Files/a b.png",
    ]);
    expect(r[0].source).toBe("markdown");
  });

  it("裸绝对路径（unix + windows 盘符）", () => {
    const r = extractLocalImagePaths(
      "见 /tmp/bug.png 与日志截图 C:\\Users\\tom\\Desktop\\err.jpg，另外 D:/pics/x.webp 也算",
    );
    expect(r.map((x) => x.path)).toEqual([
      "/tmp/bug.png",
      "C:\\Users\\tom\\Desktop\\err.jpg",
      "D:\\pics\\x.webp",
    ]);
  });

  it("file:// URL 归一为本机路径", () => {
    const r = extractLocalImagePaths("file:///Users/tom/Pic/a%20b.png 结束");
    expect(r.map((x) => x.path)).toEqual(["/Users/tom/Pic/a b.png"]);
    expect(r[0].source).toBe("file-url");
  });

  it("排除 http(s)、相对路径、非图片扩展名", () => {
    const text = [
      "远程 ![web](https://cdn.example.com/a.png) 不处理",
      "相对 assets/logo.png 不处理",
      "/tmp/readme.md 不是图片",
      "file://localhost/tmp/ok.gif 处理",
    ].join("\n");
    expect(extractLocalImagePaths(text).map((x) => x.path)).toEqual(["/tmp/ok.gif"]);
  });

  it("去重（markdown 与裸路径重复引用只取一次）", () => {
    const r = extractLocalImagePaths(
      "先 ![a](/tmp/one.png) 再贴一次 /tmp/one.png 还有 /tmp/one.png",
    );
    expect(r).toHaveLength(1);
  });

  it("每条消息最多 6 张，超出截断", () => {
    const text = Array.from({ length: 9 }, (_, i) => `/tmp/p${i}.png`).join(" ");
    expect(extractLocalImagePaths(text)).toHaveLength(6);
  });

  it("无候选返回空数组", () => {
    expect(extractLocalImagePaths("普通消息，没有路径")).toEqual([]);
  });
});

describe("MessageImages 组件三态", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    clearImageCache(); // 模块级图片缓存跨测试隔离
  });

  it("文件存在 → 渲染 <img>（data URL）", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      mime: "image/png",
      data_url: "data:image/png;base64,AAAA",
    });
    render(<MessageImages text="看 /tmp/shot.png" />);
    const img = await screen.findByRole("img");
    expect(img.getAttribute("src")).toBe("data:image/png;base64,AAAA");
    expect(vi.mocked(invoke)).toHaveBeenCalledWith("read_image_file", { path: "/tmp/shot.png" });
  });

  it("文件不在原位置 → 灰色占位提示（不是报错）", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(null);
    render(<MessageImages text="看 /tmp/gone.png" />);
    await screen.findByText(/图片已不在原位置/);
  });

  it("读取失败（超大/不支持）→ 显示后端原因", async () => {
    vi.mocked(invoke).mockRejectedValueOnce("图片过大（21 MB > 20 MB 上限）");
    render(<MessageImages text="看 /tmp/huge.png" />);
    await screen.findByText(/图片过大/);
  });

  it("同一路径复用缓存（两个组件只 invoke 一次）", async () => {
    vi.mocked(invoke).mockResolvedValue({
      mime: "image/png",
      data_url: "data:image/png;base64,AAAA",
    });
    const { unmount } = render(<MessageImages text="看 /tmp/shot.png" />);
    await screen.findByRole("img");
    unmount();
    render(<MessageImages text="再看 /tmp/shot.png" />);
    await screen.findByRole("img");
    expect(vi.mocked(invoke).mock.calls.filter(([c]) => c === "read_image_file")).toHaveLength(1);
  });

  it("无图片路径 → 不渲染任何节点", () => {
    const { container } = render(<MessageImages text="普通消息" />);
    expect(container.querySelector(".msg-images")).toBeNull();
    expect(vi.mocked(invoke)).not.toHaveBeenCalled();
  });
});
