import type { ComputeCandidateRef, ModelConnectionDraft } from './types';

export type ConnectionEditingDraft = {
  draft: ModelConnectionDraft;
  custom: boolean;
  metadataPrefillKey: string;
  secret: string;
  protectedInput: ComputeCandidateRef | null;
};

/** Credentials live only in this dialog's memory; never in a serializable UI draft. */
export class ConnectionEditingSession {
  private drafts = new Map<string, ConnectionEditingDraft>();

  get hasDrafts() { return this.drafts.size > 0; }
  stash(key: string, value: ConnectionEditingDraft) { this.drafts.set(key, value); }
  take(key: string) {
    const value = this.drafts.get(key);
    this.drafts.delete(key);
    return value;
  }
  clear(release: (input: ComputeCandidateRef) => void) {
    for (const value of this.drafts.values()) {
      value.secret = '';
      if (value.protectedInput) release(value.protectedInput);
      value.protectedInput = null;
    }
    this.drafts.clear();
  }
}
