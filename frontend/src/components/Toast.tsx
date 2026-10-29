import { useUi } from "../store/ui";

/**
 * 全局 toast 宿主（P5-T2）：样式复用 ui.css 的 .toasts/.toast（原型同款位置），
 * 点击即关；3.2s 自动消失（store/ui.ts 计时）。
 */
export default function ToastHost() {
  const toast = useUi((s) => s.toast);
  const clear = useUi((s) => s.clearToast);
  if (!toast) return null;
  return (
    <div className="toasts" data-od-id="toast-host">
      <div className="toast" role="status" onClick={clear}>
        <span>{toast}</span>
      </div>
    </div>
  );
}
