import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";

afterEach(cleanup);
import ModApplyPreview, { PathList } from "../dialogs/ModApplyPreview";
import RollbackPreviewDialog from "../dialogs/RollbackPreview";
import Confirm from "../dialogs/Confirm";
import ProgressOverlay from "../components/ProgressOverlay";
import Toast from "../components/Toast";
import { formatBytes, hiddenCount, toAppError } from "../lib/ipc";
import { statusLabel } from "../lib/format";
import type { ApplyPreview, LayerMeta, RollbackPreview } from "../lib/types";
import ProjectDetailPage, { formatLayerBytes } from "./ProjectDetailPage";
import SettingsPage from "./SettingsPage";

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(() => new Promise(() => {})), // 永不 resolve → 保持 loading
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { vi } from "vitest";

function makeLayer(seq: number, status: LayerMeta["status"]): LayerMeta {
  return {
    seq,
    id: `${String(seq).padStart(4, "0")}_x`,
    created_at: "2026-09-23T12:00:00+08:00",
    mod_src: "D:/m",
    mod_name: `mod${seq}`,
    note: null,
    status,
    structure_before: [],
    overwritten: [],
    added: [],
    stats: { overwrite_bytes: 0, add_bytes: 0 },
  };
}

const stubProject = {
  id: "p1",
  name: "G",
  base_path: "D:/G",
  created_at: "2026-09-23T12:00:00+08:00",
};

describe("ProjectDetailPage 删除层按钮", () => {
  it("rolled_back 行有删除钮，applied 行没有", () => {
    render(
      <ProjectDetailPage
        project={stubProject}
        layers={[makeLayer(2, "applied"), makeLayer(1, "rolled_back")]}
        onBack={() => {}}
        onApply={() => {}}
        onRollback={() => {}}
        onDelete={() => {}}
        onDeleteLayer={() => {}}
        busy={false}
      />,
    );
    expect(screen.getByTestId("delete-layer-1")).toBeTruthy();
    expect(screen.queryByTestId("delete-layer-2")).toBeNull();
  });
});

describe("删除层 Confirm 文案", () => {
  it("提示不可恢复", () => {
    render(
      <Confirm
        title="删除已恢复的层"
        message={`确认删除第 1 层（mod1）？\n该层备份文件将一并删除，不可恢复。`}
        confirmLabel="删除"
        onConfirm={() => {}}
        onCancel={() => {}}
      />,
    );
    expect(screen.getByText(/删除已恢复的层/)).toBeTruthy();
    expect(screen.getByText(/不可恢复/)).toBeTruthy();
  });
});

describe("formatBytes", () => {
  it("格式化字节", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(12)).toBe("12 B");
    expect(formatBytes(1024)).toBe("1.0 KB");
    expect(formatBytes(1024 * 1024 * 5)).toBe("5.0 MB");
  });
});

describe("hiddenCount", () => {
  it("截断后的剩余条数", () => {
    expect(hiddenCount(250, 200)).toBe(50);
    expect(hiddenCount(3, 3)).toBe(0);
    expect(hiddenCount(3, 99)).toBe(0);
  });
});

describe("toAppError", () => {
  it("透传结构化 AppError", () => {
    const e = toAppError({ message: "层栈为空", kind: "stack_empty", hint: "无层" });
    expect(e.kind).toBe("stack_empty");
    expect(e.hint).toBe("无层");
  });

  it("包装普通 Error", () => {
    const e = toAppError(new Error("boom"));
    expect(e.message).toBe("boom");
    expect(e.kind).toBe("other");
  });

  it("包装字符串", () => {
    expect(toAppError("oops").message).toBe("oops");
  });
});

describe("statusLabel", () => {
  it("状态中文", () => {
    expect(statusLabel("applied")).toBe("已应用");
    expect(statusLabel("rolled_back")).toBe("已恢复");
    expect(statusLabel("backed_up")).toBe("已备份");
    expect(statusLabel("restoring")).toBe("恢复中");
  });
});

describe("PathList 截断展示", () => {
  it("250 条计数 + 200 列表 → 显示另有 50", () => {
    const paths = Array.from({ length: 200 }, (_, i) => `f${i}.txt`);
    render(<PathList label="将新增" count={250} paths={paths} />);
    expect(screen.getByText(/将新增（250）/)).toBeTruthy();
    expect(screen.getByText(/另有 50 条未展示/)).toBeTruthy();
  });

  it("count=0 不渲染", () => {
    const { container } = render(<PathList label="将恢复" count={0} paths={[]} />);
    expect(container.innerHTML).toBe("");
  });
});

describe("ModApplyPreview", () => {
  const preview: ApplyPreview = {
    overwrite_count: 2,
    add_count: 1,
    untouched_count: 3,
    overwrite_bytes: 2048,
    add_bytes: 10,
    overwrite_paths: ["a.ini", "b.ini"],
    add_paths: ["c.lua"],
  };

  it("渲染统计与确认按钮", () => {
    render(
      <ModApplyPreview preview={preview} onConfirm={() => {}} onCancel={() => {}} busy={false} />,
    );
    expect(screen.getByTestId("apply-preview")).toBeTruthy();
    expect(screen.getByText(/覆盖 2 个（2.0 KB）／新增 1 个（10 B）／未触及 3 个/)).toBeTruthy();
    expect(screen.getByTestId("apply-confirm")).toBeTruthy();
    expect(screen.getByText(/请先关闭游戏/)).toBeTruthy();
  });

  it("busy 时确认钮禁用", () => {
    render(
      <ModApplyPreview preview={preview} onConfirm={() => {}} onCancel={() => {}} busy />,
    );
    expect((screen.getByTestId("apply-confirm") as HTMLButtonElement).disabled).toBe(true);
  });
});

describe("RollbackPreview", () => {
  it("渲染恢复/删除计数", () => {
    const preview: RollbackPreview = {
      layer_seq: 3,
      layer_id: "0003_x",
      restore_count: 4,
      delete_count: 5,
      empty_dirs_count: 2,
      restore_paths: ["x"],
      delete_paths: ["y"],
      empty_dirs: ["a/b", "a"],
    };
    render(
      <RollbackPreviewDialog
        preview={preview}
        onConfirm={() => {}}
        onCancel={() => {}}
        busy={false}
      />,
    );
    expect(screen.getByText(/恢复第 3 层/)).toBeTruthy();
    expect(screen.getByText(/将恢复 4 个文件／将删除 5 个本层新增文件/)).toBeTruthy();
    expect(screen.getByText(/将清理的空目录（2）/)).toBeTruthy();
  });
});

describe("Confirm", () => {
  it("purge 勾选回传", () => {
    let got: boolean | null = null;
    render(
      <Confirm
        title="删除项目"
        message="确认删除？"
        withPurge
        onConfirm={(p) => {
          got = p;
        }}
        onCancel={() => {}}
      />,
    );
    const box = screen.getByRole("checkbox") as HTMLInputElement;
    box.click();
    screen.getByTestId("confirm-ok").click();
    expect(got).toBe(true);
  });
});

describe("ProgressOverlay", () => {
  it("total=0 不确定态", () => {
    render(<ProgressOverlay progress={{ op: "apply", stage: "扫描", done: 0, total: 0 }} />);
    const fill = document.querySelector(".progress-fill")!;
    expect(fill.className).toContain("indeterminate");
    expect(screen.getByText("扫描")).toBeTruthy();
  });

  it("有分母显示 done/total", () => {
    render(<ProgressOverlay progress={{ op: "apply", stage: "备份交集", done: 3, total: 10 }} />);
    expect(screen.getByText("3 / 10")).toBeTruthy();
    expect(screen.getByText("备份交集")).toBeTruthy();
  });

  it("null 不渲染", () => {
    const { container } = render(<ProgressOverlay progress={null} />);
    expect(container.querySelector("[data-testid=progress-overlay]")).toBeNull();
  });
});

describe("formatLayerBytes 智能显示", () => {
  it("覆盖为 0 只显示新增", () => {
    expect(
      formatLayerBytes({ overwrite_bytes: 0, add_bytes: 6 * 1024 * 1024 * 1024 }),
    ).toBe("新增 6.0 GB");
  });
  it("新增为 0 只显示覆盖", () => {
    expect(formatLayerBytes({ overwrite_bytes: 2048, add_bytes: 0 })).toBe("覆盖 2.0 KB");
  });
  it("都有则 x + y", () => {
    expect(formatLayerBytes({ overwrite_bytes: 10, add_bytes: 20 })).toBe("10 B + 20 B");
  });
  it("全 0 显示 0 B", () => {
    expect(formatLayerBytes({ overwrite_bytes: 0, add_bytes: 0 })).toBe("0 B");
  });
});

describe("页面壳 .page 统一", () => {
  it("ProjectDetailPage 根节点带 page 类", () => {
    render(
      <ProjectDetailPage
        project={stubProject}
        layers={[]}
        onBack={() => {}}
        onApply={() => {}}
        onRollback={() => {}}
        onDelete={() => {}}
        onDeleteLayer={() => {}}
        busy={false}
      />,
    );
    expect(document.querySelector(".page[data-testid=detail-page]")).toBeTruthy();
  });

  it("SettingsPage 加载中标题仍常驻（header 不被整页替换）", () => {
    render(<SettingsPage onSaved={() => {}} />);
    // invoke 永不 resolve → 保持 loading
    expect(screen.getByTestId("settings-page").className).toContain("page");
    expect(screen.getByText("设置")).toBeTruthy();
    expect(screen.getByTestId("settings-loading")).toBeTruthy();
  });
});

describe("Toast", () => {
  it("错误 toast 带 hint", () => {
    render(
      <Toast
        toasts={[{ id: 1, message: "文件被占用", hint: "请关闭游戏", kind: "error" }]}
      />,
    );
    expect(screen.getByText("文件被占用")).toBeTruthy();
    expect(screen.getByText("请关闭游戏")).toBeTruthy();
    expect(document.querySelector(".toast.error")).toBeTruthy();
  });
});
