import { addDays, startOfDay, startOfWeek } from "date-fns";

export const HEATMAP_WEEKS = 16;

/** First day shown in the heatmap (a Sunday, HEATMAP_WEEKS-1 weeks back). */
export function heatmapStart(now = Date.now()): Date {
  return addDays(startOfWeek(startOfDay(now)), -7 * (HEATMAP_WEEKS - 1));
}
