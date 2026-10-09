// One in-flight poll and at most one follow-up. A refresh requested during a
// mutation must run after the earlier read; it must not reuse that stale read.
export class WorkspaceSyncLoader {
  constructor(fetchSync, fetchFull, unavailable) {
    this.fetchSync = fetchSync;
    this.fetchFull = fetchFull;
    this.unavailable = unavailable;
    this.cursor = null;
    this.snapshot = null;
    this.disabled = false;
    this.inFlight = null;
    this.queued = null;
  }

  getSnapshot() {
    if (this.inFlight) {
      if (!this.queued) {
        this.queued = this.inFlight.catch(() => {}).then(() => {
          this.queued = null;
          return this.getSnapshot();
        });
      }
      return this.queued;
    }
    this.inFlight = this.load().finally(() => { this.inFlight = null; });
    return this.inFlight;
  }

  async load() {
    if (this.disabled) return this.fetchFull();
    let reply;
    try {
      reply = await this.fetchSync(this.cursor);
    } catch (error) {
      if (!this.unavailable(error) && !(error instanceof SyntaxError)) throw error;
    }
    const collections = ["conversations", "devices", "files", "transfers"];
    const valid = reply && typeof reply.cursor === "string" && typeof reply.reset === "boolean"
      && reply.changes && typeof reply.changes === "object" && !Array.isArray(reply.changes)
      && collections.every((key) => !(key in reply.changes) || Array.isArray(reply.changes[key]))
      && (!reply.reset || ["self", "settings", "capabilities", ...collections].every((key) => key in reply.changes));
    if (!valid || (!reply.reset && !this.snapshot)) {
      this.disabled = true;
      return this.fetchFull();
    }
    const next = reply.reset ? { ...reply.changes } : { ...this.snapshot, ...reply.changes };
    this.snapshot = next;
    this.cursor = reply.cursor;
    return next;
  }
}
