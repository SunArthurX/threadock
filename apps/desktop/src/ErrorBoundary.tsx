// 全局错误边界：任何页面渲染错误只降级该区域，不再整窗黑屏
//（2026-08-15 活动页 buildHeatGrid 抛 RangeError 曾致整树卸载黑屏）
import { Component, type ReactNode } from "react";

interface State {
  error: Error | null;
}

export default class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error) {
    // eslint-disable-next-line no-console
    console.error("[ErrorBoundary]", error);
  }

  render() {
    if (this.state.error) {
      return (
        <div className="error-boundary">
          <div className="error-boundary-title">⚠ 页面渲染出错</div>
          <div className="error-boundary-msg">{this.state.error.message}</div>
          <button className="action-btn" onClick={() => this.setState({ error: null })}>
            ↻ 重试
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}
