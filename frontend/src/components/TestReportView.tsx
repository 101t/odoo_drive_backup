import { CircleCheck, CircleX, TriangleAlert } from "lucide-react";
import type { TestReport } from "../api/types";

export function TestReportView({ report }: { report: TestReport }) {
  return (
    <ol className="space-y-3">
      {report.steps.map((step, index) => (
        <li key={index} className="flex gap-3">
          {step.status === "ok" && <CircleCheck className="mt-0.5 size-5 shrink-0 text-emerald-500" />}
          {step.status === "warn" && <TriangleAlert className="mt-0.5 size-5 shrink-0 text-amber-500" />}
          {step.status === "fail" && <CircleX className="mt-0.5 size-5 shrink-0 text-rose-500" />}
          <div className="min-w-0">
            <p className="text-sm font-medium">{step.name}</p>
            <p className="break-words text-sm text-zinc-500 dark:text-zinc-400">{step.detail}</p>
          </div>
        </li>
      ))}
    </ol>
  );
}
