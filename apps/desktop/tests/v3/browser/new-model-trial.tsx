import { createRoot } from 'react-dom/client';
import { mockIPC } from '@tauri-apps/api/mocks';
import { Models } from '../../../src/features/models/Models';
import { PlanEditor } from '../../../src/plan-editor';
import { PresentationRoot, ProductPage } from '../../../src/ui';
import { readyManagement, dailyPlan, mockProductInvoke } from './product-fixtures';
import type { ModelFeedStatus } from '../../../src/features/model-feed/types';
import type { TrustReport } from '../../../src/features/trust/types';
import '../../../src/occami/styles.css';
const params = new URLSearchParams(location.search);
const language = params.get('lang') === 'zh' ? 'zh' : 'en';
const trace: { command: string; payload: any }[] = [];
Object.assign(window, { trialTrace: trace });
const feed: ModelFeedStatus = { retrieval_enabled:true, held:{sequence:12, issued_at:'2026-10-18T00:00:00Z', retrieved_at:1792281600000}, highest_accepted_sequence:12, last_attempt:{at:1792281600000,result:'ok',reason:null}, verification:{kind:'verified'}, counts:{applied:4,ignored_embedded:2,newer_client:1,unresolved:0}, next_automatic_attempt_at:null, disclosure:'Model information is retrieved automatically from hiroute.ai without account or usage identifiers. You can turn off automatic retrieval.', disclosure_acknowledged:false };
const counts = { executed:58,complete:41,meets_floor:37,partial:4,awaiting_assessment:1,unscored:12,sessions:9 };
const report: TrustReport = { plan_id:dailyPlan.agent_plan_id, active_revision:8, revision:'active',from_ms:0,to_ms:Date.now(), rows:[{branch_id:'smart_saving',group:'regular',model_configuration_id:'model.trial',reasoning_profile_id:'high',attribution:'single',executor:null,identity_stability:'rolling-alias',counts,competence_rate:37/41,scoring_coverage:41/58,execution_share:0.8,protections:[],availability_relays:0,duration_p50_ms:1500,duration_p90_ms:3500,output_tokens_per_second:50,task_outcomes:{submitted:3,succeeded:2,failed:1}}], branches:[{branch_id:'smart_saving',threshold_millis:720,full_history:counts,upward_cohort:counts,upward_from_ms:0,suggestion:{kind:'step_up',threshold_millis:650},replay:{included:58,excluded_by_reason:{heuristic:2},recorded_share_at_current:0.8,estimate_only:true,thresholds:[{threshold_millis:650,regular:49,primary:9,protected_as_primary:3,regular_share:49/58,newly_regular:7}]}}],sessions:[],session_total:9,announcements:[{model_configuration_id:'model.trial',included_on:'2026-10-10',before:31,announcement_day:2,after:25,executed_version_verified:false}],cost:{kind:'unavailable',reason:'comparison basis not available'} };
let failed = false;
mockIPC(async (command, payload: any) => {
  trace.push({command,payload});
  if (command === 'model_feed_status') { if (params.has('error') && !failed) {failed=true;throw new Error('DAEMON_UNAVAILABLE');} return structuredClone(feed); }
  if (command === 'model_feed_refresh') { feed.last_attempt={at:Date.now(),result:'signature',reason:'invalid_signature'}; return structuredClone(feed); }
  if (command === 'model_feed_settings_apply') { if (payload.input.retrieval_enabled !== undefined) feed.retrieval_enabled=payload.input.retrieval_enabled; if (payload.input.acknowledge_disclosure) feed.disclosure_acknowledged=true; return structuredClone(feed); }
  if (command === 'model_feed_remove_preview') return { sources:[{source_id:'source/feed',display_name:'Team experimental model connection with a deliberately long readable name',model_configuration_ids:['model.trial'],published_revisions:[]}],published_revisions:[{plan_id:'plan/frozen',content_revision:3}],retained_sequence:12,retrieval_enabled:feed.retrieval_enabled,digest:'sha256:preview' };
  if (command === 'model_feed_remove_apply') { if (params.has('stale')) throw new Error('CHANGE_PREVIEW_STALE'); feed.held=null; feed.verification={kind:'none'}; if (payload.input.also_disable_retrieval) feed.retrieval_enabled=false; return structuredClone(feed); }
  if (command === 'model_availability') return {featured:[],models:[]};
  if (command === 'observation_read' && payload.request.intent.view === 'plan_trust') {
    const value=structuredClone(report); if (params.has('empty')) {value.rows=[]; value.branches[0].full_history={executed:0,complete:0,meets_floor:0,partial:0,awaiting_assessment:0,unscored:0,sessions:0};}
    if (payload.request.intent.query.group_by === 'executor') value.rows.forEach(row => row.executor='deepseek_harness');
    return value;
  }
  if (command === 'observation_read') return {samples:[],summary:{models:[],scored_stage_count:0,unrated_stage_count:0,session_count:0,available_revisions:[8]},next_cursor:null};
  return mockProductInvoke(command,payload,'ready');
});
const plan = structuredClone(dailyPlan);
plan.desired.delegation_enabled=false;
plan.desired.strategy.judgment!.degree.simple_threshold_millis=720;
plan.desired.strategy.classifier={kind:'decision_service',service:{id:'decision/test',revision:1,name:'Decision model',connection:{kind:'custom',endpoint:'https://decision.example.test',timeout_ms:1000,auth_header:{kind:'none'}}}};
createRoot(document.getElementById('root')!).render(<PresentationRoot language={language} theme={params.get('theme') === 'dark' ? 'dark' : 'light'} textScale={1}>
  {params.get('view') === 'editor' ? <ProductPage title="Routing"><PlanEditor plan={plan} language={language} onDone={async()=>{}} onClose={()=>{}} /></ProductPage> : <ProductPage title="Models" flush className="hr-models-page"><Models language={language} snapshot={readyManagement} mutable onRefresh={async()=>{}} onPrepareKeyInput={async()=>{throw Error('outside scenario')}} onPreview={async()=>{throw Error('outside scenario')}} onApply={async()=>''} onDiscardProtectedInputs={()=>{}} /></ProductPage>}
</PresentationRoot>);
