import categoryV1 from '../../../../../crates/domain/src/routing/fixtures/native_category_v1.json' with { type: 'json' };
import stock from '../../../../../crates/domain/src/routing/fixtures/system_one_c_v1.json' with { type: 'json' };
import { defaultJudgment, type Judgment, type RouteBranch, type DecisionService } from './types.ts';

export const C_PROTOCOL = 'hiroute.system-one-c/v1';
export const LEGACY_PROTOCOL = 'hiroute.system-one-legacy/v1';
export type NativeProtocol = { schema: string; prior_context_max_turns: number };
type ResolvedNativeProtocol = NativeProtocol & { fixed: { degree_prefix: string; assessment_wrapper: string; applicability: typeof stock.q2 } };
export type FrozenNativeProtocol = { protocol: ResolvedNativeProtocol; context_layout: string; decision: Record<string, unknown>; scopes: Record<string, { assessment: ReturnType<typeof composedAssessment>; simple_threshold_millis: number; competence_floor_millis: number; standard: string }>; digest: string };
export type CapabilityShape = 'standard' | 'max_branches';
export type CCapability = { schema: string; state: 'verified' | 'unsupported' | 'not_tested' | 'test_failed'; service_id: string; service_revision: number; endpoint_digest: string; model: string; protocol_digest: string; shape: CapabilityShape; question_count: number; tested_at_unix_ms: number; duration_millis: number; reason: string };
export type CTestRequest = { schema: string; service_id: string; service_revision: number; protocol: FrozenNativeProtocol; shape: CapabilityShape };

const wrapper = stock.q1.instructions.split(' Distinguish those meanings')[0];
const legacyWrapper = 'Evaluate only the completed stage from state.visible_conversation[state.assessment_from] through the end. state.latest_user may supply explicit feedback, but is not yet executed.';
export function nativeProtocol(): NativeProtocol {
  return { schema: C_PROTOCOL, prior_context_max_turns: 8 };
}
function resolvedProtocol(protocol: NativeProtocol): ResolvedNativeProtocol {
  return { ...protocol, fixed: { degree_prefix: 'Assess state.latest_user only, not historical competence.', assessment_wrapper: wrapper, applicability: structuredClone(stock.q2) } };
}
export function assessmentStandard(judgment: Judgment): 'default' | 'custom' {
  return judgment.competence.standard ?? (judgment.competence.instructions === defaultJudgment.competence.instructions && judgment.competence.criteria.every((c, i) => c === defaultJudgment.competence.criteria[i]) ? 'default' : 'custom');
}
export function stockAssessment(judgment: Judgment, protocol?: NativeProtocol): Judgment {
  const result = structuredClone(judgment);
  result.competence.standard = 'default';
  result.competence.instructions = protocol ? stock.q1.instructions.slice(wrapper.length + 1) : defaultJudgment.competence.instructions;
  result.competence.criteria = structuredClone(protocol ? stock.q1.criteria : defaultJudgment.competence.criteria) as [string, string, string];
  return result;
}
export function resolveAssessment(judgment: Judgment, protocol?: NativeProtocol): Judgment {
  if (assessmentStandard(judgment) === 'default') return stockAssessment(judgment, protocol);
  const result = structuredClone(judgment);
  if (protocol || result.competence.standard) result.competence.standard = 'custom';
  return result;
}
export function composedAssessment(protocol: NativeProtocol | undefined, judgment: Judgment) {
  const j = resolveAssessment(judgment, protocol);
  return { type: 'score', instructions: `${protocol ? wrapper : legacyWrapper} ${j.competence.instructions}`, criteria: j.competence.criteria };
}
function canonical(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0).map(([k, v]) => [k, canonical(v)]));
  return value;
}
export async function contentDigest(value: unknown): Promise<string> {
  const hash = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(JSON.stringify(canonical(value))));
  return 'sha256:' + Array.from(new Uint8Array(hash), b => b.toString(16).padStart(2, '0')).join('');
}
export function nativeReferenceIssue(c: boolean, text: string): boolean {
  return (c ? ['visible_conversation', 'assessment_from'] : ['state.assessment_stage', 'state.prior_context']).some(field => text.includes(field));
}
export async function freezeNative(protocol: NativeProtocol, judgment: Judgment, branches?: RouteBranch[], defaultId?: string): Promise<FrozenNativeProtocol> {
  const effective = resolveAssessment(judgment, protocol);
  const branchJudgment = (b: RouteBranch) => resolveAssessment(b.judgment ?? judgment, protocol);
  const degree = (j: Judgment) => ({ instructions: j.degree.instructions, levels: [{ id: 'simple', criterion: j.degree.simple }, { id: 'complex', criterion: j.degree.complex }] });
  const decision = branches ? {
    kind: 'categorical', instructions: `${categoryV1.category_prefix}${defaultId}${categoryV1.category_suffix}`,
    options: branches.map(b => ({ id: b.id, criterion: b.condition, ...(b.primary_candidates.length ? { refinement: { kind: 'ordinal', ...degree(branchJudgment(b)), instructions: `${categoryV1.degree_prefix}${b.name}${categoryV1.degree_condition_separator}${b.condition}${categoryV1.degree_suffix}${branchJudgment(b).degree.instructions}` } } : {}) })),
  } : { kind: 'ordinal', ...degree(effective) };
  const scopes = Object.fromEntries((branches ? branches.map(b => [b.id, branchJudgment(b)] as const) : [['smart_saving', effective] as const]).map(([id, j]) => [id, {
    assessment: composedAssessment(protocol, j), simple_threshold_millis: j.degree.simple_threshold_millis, competence_floor_millis: j.competence.floor_millis,
    standard: assessmentStandard(j),
  }]));
  const context_layout = 'hiroute.partitioned-history/v1', resolved = resolvedProtocol(protocol);
  return { protocol: resolved, context_layout, decision, scopes, digest: await contentDigest([resolved, context_layout, decision, scopes]) };
}
export async function stockProbe(service: DecisionService, shape: CapabilityShape): Promise<CTestRequest> {
  const protocol = nativeProtocol(), judgment = stockAssessment(defaultJudgment, protocol);
  const branches = shape === 'max_branches' ? Array.from({ length: 16 }, (_, i): RouteBranch => ({ id: `probe-${i + 1}`, name: `probe-${i + 1}`, condition: `Synthetic category ${i + 1}`, candidates: [], primary_candidates: [{ binding_id: 'synthetic' }] })) : undefined;
  return { schema: 'hiroute.historical-applicability-test/v1', service_id: service.id, service_revision: service.revision, protocol: await freezeNative(protocol, judgment, branches, branches?.[0].id), shape };
}
