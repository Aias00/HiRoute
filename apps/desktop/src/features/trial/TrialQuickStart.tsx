import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Dialog } from '../../ui';
import { requestEditorReplacement } from '../../ui/discard-guard';
import { emptyEditor, type Editor, type Options, type Plan, type Selection } from '../../plan-editor';
import type { DesktopOperation, DesktopSnapshot } from '../../product/home-projections';
import { ModelManagementPage } from '../../product/ModelManagementPage';
import type { ManagementSnapshot } from '../models/types';
import type { OperationReference } from '../model-connections/types';
import type { AvailableModel, ModelAvailabilityResult } from '../model-feed/types';
import type { AgentSnapshot } from '../agents/types';
import { agentDisplayName } from '../agents/ecosystems';
import { DecisionSelector } from '../decision-services/DecisionSelector';
import { DecisionServicesPage } from '../decision-services/DecisionServicesPage';
import { emptyService, type Classifier, type DecisionService } from '../decision-services/types';
import type { DecisionIntent, OpenDecisionConnection } from '../decision-services/presentation';
import { trustLevels } from '../trust/levels';
import { TrialConnectModel } from './TrialConnectModel';
import { TrialAgentChoice, TrialAgentMapping } from './TrialAgentMapping';
import { TrialReview } from './TrialReview';
import { currentPrimary, initialTrialMapping, trialSelection, type TrialMapping } from './model';
import './trial.css';

export function TrialQuickStart({ initialModelId, language, mutable, snapshot, operation, onOperation, onRefresh, onClose, onOpenAgent, onOpenPerformance, onOpenSessions }: {
  initialModelId?: string; language: 'zh' | 'en'; mutable: boolean; snapshot: DesktopSnapshot | null; operation: DesktopOperation | null;
  onOpenPerformance?(plan: Plan): void; onOpenSessions?(): void;
  onOperation(operation: DesktopOperation | null): void; onRefresh(): Promise<void>; onClose(): void; onOpenAgent(id: string): void;
}) {
  const t = (zh: string, en: string) => language === 'zh' ? zh : en;
  const [data, setData] = useState<{ availability: ModelAvailabilityResult; options: Options; management: ManagementSnapshot; agents: AgentSnapshot } | null>(null);
  const [trialId, setTrialId] = useState(initialModelId ?? '');
  const [primaryId, setPrimaryId] = useState('');
  const [primarySelection, setPrimarySelection] = useState<Selection>();
  const [trialBinding, setTrialBinding] = useState(''), [primaryBinding, setPrimaryBinding] = useState('');
  const [mapping, setMapping] = useState<TrialMapping>({ agentId: '', presets: ['opus'] });
  const [classifier, setClassifier] = useState<Classifier>(() => ({ kind: 'decision_service', service: emptyService() }));
  const [services, setServices] = useState<DecisionService[]>([]);
  const [connect, setConnect] = useState<AvailableModel | null>(null), [subscriptions, setSubscriptions] = useState(false);
  const [decision, setDecision] = useState<DecisionIntent | null>(null);
  const decisionSelect = useRef<((service: DecisionService) => void) | null>(null);
  const [savedConnections, setSavedConnections] = useState<string[]>([]);
  const [persistencePending, setPersistencePending] = useState(false);
  const [review, setReview] = useState<Editor | null>(null), [published, setPublished] = useState<Plan | null>(null);
  const [loading, setLoading] = useState(true), [error, setError] = useState('');
  const initialized = useRef(false);
  const inFlight = useRef(false);
  const [alias] = useState(() => `trial-${crypto.randomUUID().replaceAll('-', '').slice(0, 16)}`);
  async function refresh() {
    if (inFlight.current) return;
    inFlight.current = true; setLoading(true); setError('');
    try {
      const [availability, options, management, agents, decisions] = await Promise.all([
        invoke<ModelAvailabilityResult>('model_availability'),
        invoke<Options>('plan_editor_options', { input: { requirements: {}, native_selections: {}, suggest_free: false } }),
        invoke<ManagementSnapshot>('compute_management_snapshot'), invoke<AgentSnapshot>('agent_snapshot'),
        invoke<{ services: DecisionService[] }>('decision_services'),
      ]);
      setData({ availability, options, management, agents }); setServices(decisions.services);
      if (!initialized.current) {
        initialized.current = true;
        const target = initialTrialMapping(agents); setMapping(target);
        const current = currentPrimary(agents.agents.find(a => a.agent_id === target.agentId), agents.plans.plans);
        const candidate = options.candidates.find(c => c.binding_id === current?.binding_id && c.routable);
        if (candidate) { setPrimaryId(candidate.model_configuration_id); setPrimaryBinding(candidate.binding_id); setPrimarySelection(current); }
        if (decisions.services.length === 1) setClassifier({ kind: 'decision_service', service: decisions.services[0] });
      }
      return { availability, options, management, agents };
    } catch { setError(t('试用所需信息暂时无法读取。请重试，已完成的配置仍保留。', 'Trial setup information could not be read. Retry; completed configuration is retained.')); }
    finally { inFlight.current = false; setLoading(false); }
  }
  useEffect(() => { void refresh(); }, []);
  const openDecision: OpenDecisionConnection = (kind, select) => { decisionSelect.current = select; setDecision({ key: crypto.randomUUID(), kind }); };
  const operationRef = (value: OperationReference) => onOperation({ ...value, safe_error_code: null });
  const models = data?.availability.models ?? [];
  const trial = models.find(m => m.model_configuration_id === trialId), primary = models.find(m => m.model_configuration_id === primaryId);
  function candidates(id: string) { return data?.options.candidates.filter(c => c.model_configuration_id === id && c.routable) ?? []; }
  const trialCandidates = candidates(trialId), primaryCandidates = candidates(primaryId);
  const selectedTrial = trialCandidates.find(c => c.binding_id === trialBinding) ?? (trialCandidates.length === 1 ? trialCandidates[0] : undefined);
  const selectedPrimary = primaryCandidates.find(c => c.binding_id === primaryBinding) ?? (primaryCandidates.length === 1 ? primaryCandidates[0] : undefined);
  const ready = !!selectedTrial && !!selectedPrimary && trialId !== primaryId && classifier.kind === 'decision_service' && !!classifier.service.name && !!mapping.agentId && mapping.presets.length > 0 && !loading && !error;
  function modelField(role: 'trial' | 'primary') {
    const model = role === 'trial' ? trial : primary;
    const id = role === 'trial' ? trialId : primaryId;
    const available = role === 'trial' ? trialCandidates : primaryCandidates;
    const selected = role === 'trial' ? selectedTrial : selectedPrimary;
    return <section className="editor-section"><label className="field"><span className="field-label">{role === 'trial' ? t('试用模型（常规）', 'Trial model (Regular)') : t('主力模型', 'Primary model')}</span><select className="input" value={id} onChange={e => {
      if (role === 'trial') { setTrialId(e.target.value); setTrialBinding(''); }
      else { setPrimaryId(e.target.value); setPrimaryBinding(''); setPrimarySelection(undefined); }
    }}><option value="">{t('选择模型', 'Select a model')}</option>{models.filter(m => m.origin.kind !== 'published').map(m => <option key={m.model_configuration_id} value={m.model_configuration_id} disabled={m.availability.kind === 'needs_newer_client'}>{m.display_name}{m.availability.kind === 'needs_newer_client' ? ` · HiRoute ${m.availability.version}+` : ''}</option>)}</select></label>
      {model && available.length > 1 && <label className="field"><span className="field-label">{t('使用的接入', 'Connection')}</span><select className="input" value={selected?.binding_id ?? ''} onChange={e => role === 'trial' ? setTrialBinding(e.target.value) : (setPrimaryBinding(e.target.value), setPrimarySelection(undefined))}><option value="">{t('请选择', 'Select')}</option>{available.map(c => <option key={c.binding_id} value={c.binding_id}>{data?.management.sources.find(s => s.models.some(m => m.binding_id === c.binding_id))?.display_name ?? c.display_name} · {c.billing_class}</option>)}</select></label>}
      {model && !available.length && model.availability.kind !== 'needs_newer_client' && <button className="btn" onClick={() => setConnect(model)}>{t('在此接入', 'Connect here')} {model.display_name}</button>}
      {role === 'primary' && <><p className="field-help">{t('仅在已保存配置明确指向单个模型时，才自动填入当前主力；原生预设名称不会被猜成某个模型版本。', 'The current Primary is prefilled only when saved settings identify a single model. A native preset name does not prove a model version.')}</p><button className="btn btn-quiet" onClick={() => setSubscriptions(true)}>{t('复用本机订阅', 'Reuse a local subscription')}</button></>}
      {model && ['rolling-alias', 'dynamic-route', 'unknown'].includes(model.identity_stability ?? 'unknown') && <p className="field-help">{t('供应商可能更新同名模型；后续证据可能混合多个版本。', 'The provider may update this model alias; later evidence may mix versions.')}</p>}
    </section>;
  }
  return <Dialog open title={t('试用新模型', 'Try a new model')} description={t('保留主力保护，从自己的任务中积累证据。', 'Keep Primary protection and build evidence from your own tasks.')} closeLabel={t('关闭', 'Close')} onClose={() => void (async () => { if (persistencePending || await requestEditorReplacement('routing')) onClose(); })()} className="trial-dialog">
    {error && <div className="callout warn" role="alert"><p>{error}</p><button className="btn" onClick={() => void refresh()}>{t('重试', 'Retry')}</button></div>}
    {savedConnections.length > 0 && <p role="status">{t('本次已保存接入：', 'Connections saved in this flow: ')}{savedConnections.join(' · ')}</p>}
    {published ? <TrialAgentMapping onOpenPerformance={() => onOpenPerformance?.(published)} onOpenSessions={onOpenSessions} plan={published} initial={mapping} language={language} mutable={mutable} onOperation={operationRef} onChanged={() => void onRefresh()} onOpenAgent={onOpenAgent} /> : review ? <><p>{t('将映射', 'Mapping to review')}: {agentDisplayName(mapping.agentId, language)} · {mapping.presets.join(', ')}</p><TrialReview editor={review} language={language} mutable={mutable} snapshot={snapshot} operation={operation} onOperation={onOperation} onRefresh={onRefresh} onPublished={setPublished} onPendingChange={setPersistencePending} onClose={onClose} onOpenServices={openDecision} /></> : connect && data ? <TrialConnectModel key={connect.model_configuration_id} model={connect} language={language} management={data.management} mutable={mutable} onOperation={operationRef} onBack={() => { setConnect(null); void refresh(); }} onSaved={(_result, model) => { setSavedConnections(values => [...new Set([...values, model.display_name])]); setConnect(null); void refresh(); void onRefresh(); }} /> : data ? <fieldset disabled={loading || !mutable}>
      {modelField('trial')}{modelField('primary')}
      <DecisionSelector classifier={classifier} smart allowHeuristic={false} services={services} language={language} onChange={setClassifier} onOpenServices={openDecision} />
      <TrialAgentChoice snapshot={data.agents} value={mapping} onChange={setMapping} language={language} />
      <p>{t('起始档位为“试用”0.90。完整评分、评分覆盖率和会话分布会影响升档建议；保持当前档位也是正常结果。', 'Start at Trial 0.90. Complete scores, scoring coverage and session diversity inform upward suggestions. Staying at this level is a normal outcome.')}</p>
      <div className="actions"><button className="btn" onClick={() => void refresh()}>{t('刷新可用状态', 'Refresh availability')}</button><button className="btn btn-primary" disabled={!ready} onClick={() => {
        if (!ready || !selectedTrial || !selectedPrimary || classifier.kind !== 'decision_service') return;
        const editor = emptyEditor(); editor.mode = 'smart_saving'; editor.custom_alias = alias;
        editor.display_name = `${trial!.display_name} + ${primary!.display_name}`;
        editor.purpose = t('在主力保护下试用新模型，根据自己的任务证据调整路由。', 'Try a new model with Primary protection and adjust routing using evidence from your own tasks.');
        editor.smart.economy = [trialSelection(selectedTrial)]; editor.smart.primary = [primarySelection?.binding_id === selectedPrimary.binding_id ? primarySelection : trialSelection(selectedPrimary)];
        editor.smart.judgment.competence.criteria[0] = 'The trial model made no effective progress, repeatedly failed without recovery, or the latest user message explicitly identifies its completed result as wrong. Judge the referenced completed stage and distinguish a new requirement from a correction; repetition, continuation or silence alone is not failure.';
        editor.smart.classifier = structuredClone(classifier); editor.smart.judgment.degree.simple_threshold_millis = trustLevels[0].threshold_millis;
        setReview(editor);
      }}>{t('核对路由与映射', 'Review route and mapping')}</button></div>
    </fieldset> : <p role="status">{t('正在读取配置…', 'Reading configuration…')}</p>}
    {decision && <Dialog open title={t('接入决策连接', 'Connect a decision service')} closeLabel={t('返回试用', 'Back to trial')} onClose={() => void (async () => { if (await requestEditorReplacement('decisions')) setDecision(null); })()} className="trial-dialog"><DecisionServicesPage language={language} active mutable={mutable} intent={decision} plans={snapshot?.catalog.plans} drafts={snapshot?.catalog.drafts} onChanged={() => void refresh()} onReturn={() => setDecision(null)} onUse={service => { decisionSelect.current?.(service); setDecision(null); void refresh(); }} /></Dialog>}
    {subscriptions && <Dialog open title={t('复用本机订阅', 'Reuse a local subscription')} closeLabel={t('返回试用', 'Back to trial')} onClose={() => void (async () => { if (await requestEditorReplacement('models')) { setSubscriptions(false); void refresh(); } })()} className="trial-dialog"><ModelManagementPage language={language} active trustedAuthority={mutable} startScanning onOperation={operationRef} onRecoveryRefresh={() => void refresh()} onChanged={() => { void refresh(); void onRefresh(); }} plans={snapshot?.catalog.plans} agents={data?.agents.agents} /></Dialog>}
  </Dialog>;
}
