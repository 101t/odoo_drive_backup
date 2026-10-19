import { createContext, useCallback, useContext, useRef, useState, type ReactNode } from "react";
import { Button, Checkbox, Modal } from "./ui";

export interface ConfirmOptions {
  title: string;
  message?: ReactNode;
  confirmLabel?: string;
  danger?: boolean;
  /** Optional extra checkbox (e.g. "also delete the files"). */
  checkbox?: string;
  checkboxDefault?: boolean;
}

export interface ConfirmResult {
  confirmed: boolean;
  checked: boolean;
}

type ConfirmFn = (options: ConfirmOptions) => Promise<ConfirmResult>;

const ConfirmContext = createContext<ConfirmFn | null>(null);

export function ConfirmProvider({ children }: { children: ReactNode }) {
  const [options, setOptions] = useState<ConfirmOptions | null>(null);
  const [checked, setChecked] = useState(false);
  const resolver = useRef<((result: ConfirmResult) => void) | null>(null);

  const confirm = useCallback<ConfirmFn>((opts) => {
    setOptions(opts);
    setChecked(opts.checkboxDefault ?? false);
    return new Promise<ConfirmResult>((resolve) => {
      resolver.current = resolve;
    });
  }, []);

  const close = (confirmed: boolean) => {
    resolver.current?.({ confirmed, checked });
    resolver.current = null;
    setOptions(null);
  };

  return (
    <ConfirmContext.Provider value={confirm}>
      {children}
      <Modal
        open={options !== null}
        onClose={() => close(false)}
        title={options?.title ?? ""}
        size="sm"
        footer={
          <>
            <Button onClick={() => close(false)}>Cancel</Button>
            <Button variant={options?.danger ? "danger" : "primary"} onClick={() => close(true)} autoFocus>
              {options?.confirmLabel ?? "Confirm"}
            </Button>
          </>
        }
      >
        <div className="space-y-4 text-sm text-zinc-600 dark:text-zinc-300">
          {options?.message}
          {options?.checkbox && <Checkbox checked={checked} onChange={setChecked} label={options.checkbox} />}
        </div>
      </Modal>
    </ConfirmContext.Provider>
  );
}

export function useConfirm(): ConfirmFn {
  const ctx = useContext(ConfirmContext);
  if (!ctx) throw new Error("useConfirm must be used inside ConfirmProvider");
  return ctx;
}
