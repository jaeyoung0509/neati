import type { ScanGapKind, ScanResult } from '../models/types';

const gapLabels: Record<ScanGapKind, string> = {
  full_disk_access: 'Protected locations may need Full Disk Access; file permissions can also deny access',
  permission_denied: 'Access was denied; check file ownership and permissions',
  tool_missing: 'Required tool is unavailable; install or select it before scanning again',
  unsupported_adapter: 'This inspection is unsupported by the current adapter',
  safety_protected: 'Protected entries were excluded by the safety policy',
  owner_state_unknown: 'The owning app’s activity could not be verified; try scanning again',
  depth_limit: 'Locations reached the scan depth limit',
  cancelled: 'Locations were not finished before the scan stopped',
  io_error: 'Locations could not be read because of an I/O error',
  unknown: 'The inspection failure’s cause is unknown; try scanning again',
};

/** Backend diagnostics only. These labels never authorize cleanup. */
export function scanInspectionDetails(scan: Pick<ScanResult, 'gaps'>) {
  return (scan.gaps ?? []).filter(gap => gap.count > 0).map(gap => ({
    kind: gap.kind, count: gap.count, label: gapLabels[gap.kind],
  }));
}
