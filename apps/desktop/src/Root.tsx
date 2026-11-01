import { Dashboard } from "./dashboard/Dashboard";
import { useMobileLayout } from "./lib/platform";
import { MobileApp } from "./mobile/MobileApp";
import { Picker } from "./picker/Picker";

/** Picker window, phone layout, or the desktop dashboard. */
export function Root({ picker }: { picker: boolean }) {
  const mobile = useMobileLayout();
  if (picker) return <Picker />;
  return mobile ? <MobileApp /> : <Dashboard />;
}
