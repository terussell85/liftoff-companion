import { Badge } from "@/components/ui/badge";

/** Badge for a race session's segmentation method (how it was detected). */
export function SegBadge({ method }: { method: string }) {
  if (method === "gamelog+telemetry") {
    return (
      <Badge variant="success" className="font-mono text-[10px]">
        fused
      </Badge>
    );
  }
  if (method === "telemetry") {
    return (
      <Badge variant="warn" className="font-mono text-[10px]">
        telemetry
      </Badge>
    );
  }
  return (
    <Badge variant="secondary" className="font-mono text-[10px]">
      gamelog
    </Badge>
  );
}
