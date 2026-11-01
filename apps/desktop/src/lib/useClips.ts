import { useCallback, useEffect, useRef, useState } from "react";
import { api, errorMessage } from "./api";
import type { ClipSummary, ListQuery } from "./types";

const PAGE = 80;

/** Keyset-paginated clip list for a query. `query` must be memoized. */
export function useClips(query: ListQuery, pageSize = PAGE) {
  const [items, setItems] = useState<ClipSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [hasMore, setHasMore] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const gen = useRef(0);
  const loadingMore = useRef(false);
  const itemsRef = useRef(items);
  useEffect(() => {
    itemsRef.current = items;
  }, [items]);

  const first = useCallback(async () => {
    const g = ++gen.current;
    setLoading(true);
    try {
      const rows = await api.listClips({ ...query, limit: pageSize });
      if (g !== gen.current) return;
      setItems(rows);
      setHasMore(rows.length === pageSize);
      setError(null);
    } catch (e) {
      if (g === gen.current) setError(errorMessage(e));
    } finally {
      if (g === gen.current) setLoading(false);
    }
  }, [query, pageSize]);

  useEffect(() => {
    void first();
  }, [first]);

  const loadMore = useCallback(async () => {
    const cur = itemsRef.current;
    if (loadingMore.current || !hasMore || cur.length === 0) return;
    loadingMore.current = true;
    const g = gen.current;
    try {
      const last = cur[cur.length - 1];
      const rows = await api.listClips({
        ...query,
        before: last.last_seen_at,
        before_id: last.id,
        limit: pageSize,
      });
      if (g !== gen.current) return;
      setItems((prev) => {
        const ids = new Set(prev.map((x) => x.id));
        return [...prev, ...rows.filter((r) => !ids.has(r.id))];
      });
      setHasMore(rows.length === pageSize);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      loadingMore.current = false;
    }
  }, [query, hasMore, pageSize]);

  /** Re-fetch the newest page and merge it with what's loaded. */
  const refresh = useCallback(async () => {
    const g = gen.current;
    try {
      const rows = await api.listClips({ ...query, limit: pageSize });
      if (g !== gen.current) return;
      setItems((prev) => {
        if (rows.length === 0) return [];
        const ids = new Set(rows.map((r) => r.id));
        const oldest = rows[rows.length - 1].last_seen_at;
        const tail = rows.length < pageSize ? [] : prev.filter((x) => !ids.has(x.id) && x.last_seen_at < oldest);
        return [...rows, ...tail];
      });
      if (rows.length < pageSize) setHasMore(false);
    } catch {
      /* keep what we have */
    }
  }, [query, pageSize]);

  const patch = useCallback((id: string, p: Partial<ClipSummary>) => {
    setItems((prev) => prev.map((x) => (x.id === id ? { ...x, ...p } : x)));
  }, []);

  return { items, loading, hasMore, error, loadMore, refresh, reload: first, patch };
}
