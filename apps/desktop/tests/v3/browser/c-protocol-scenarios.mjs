import { branchRouting } from '../../../src/features/decision-services/types.ts';
import { composedAssessment, nativeProtocol, stockAssessment } from '../../../src/features/decision-services/native-protocol.ts';
import { scenario } from './scenario-selection.mjs';

export function cProtocolScenarios({ fresh, routing, click, until, all, text, calls, setInput, control }) {
  const assert = (value, message) => { if (!value) throw new Error(message); };
  const choose = (select, value) => { select.value = value; select.dispatchEvent(new Event('change', { bubbles: true })); };
  const service = { id: 'decision/c-ui-fixture', revision: 7, name: 'Saved applicability fixture', connection: {
    kind: 'system_one', provider: 'compatible', model: 'synthetic-c-model', endpoint: 'https://decision.example/systemone', timeout_ms: 3000,
    auth_header: { name: 'Authorization', value_secret_ref: 'decision/c-ui-fixture/r7' },
  } };
  const source = { plan: { workspace_id: 'default', plan_id: 'plan/fixture', content_revision: 1, content_digest: 'sha256:' + 'a'.repeat(64) }, scope: 'default' };
  async function open(language, { c = false, branches = false, parkedBranches = false, same = false } = {}) {
    await fresh(() => {
      control().handlers.decision_services = () => ({ services: [service] });
      control().handlers.desktop_snapshot = payload => {
        const snapshot = structuredClone(control().fixtureResponse('desktop_snapshot', payload));
        const plan = snapshot.catalog.plans[0], strategy = plan.desired.strategy;
        strategy.classifier = { kind: 'decision_service', service };
        if (c) { strategy.native_protocol = nativeProtocol(); strategy.judgment = stockAssessment(strategy.judgment, strategy.native_protocol); }
        plan.desired.delegation_enabled = false;
        const custom = branchRouting(service);
        custom.branches.forEach((b,i) => { b.name = 'Category ' + i; b.condition = 'Work of category ' + i; b.candidates = structuredClone(strategy.economy); b.primary_candidates = structuredClone(strategy.primary); b.judgment = structuredClone(strategy.judgment); });
        custom.judgment = structuredClone(strategy.judgment);
        if (c) custom.native_protocol = nativeProtocol();
        if (branches) { plan.desired.mode = 'custom_branches'; plan.desired.strategy = { mode: 'branches', routing: custom }; }
        if (same) plan.assessment_transition = { smart_saving: { protocol: c ? 'hiroute.system-one-c/v1' : 'hiroute.system-one-legacy/v1', method: 'stock', assessment_digest: 'sha256:' + 'b'.repeat(64) } };
        if (parkedBranches) {
          delete custom.native_protocol;
          custom.branches.forEach(b => { b.judgment = null; });
          const smart = structuredClone(strategy); delete smart.mode;
          snapshot.catalog.drafts = [{ schema:'hiroute.plan-draft/v1', workspace_id:'default', draft_id:'draft/transition-ui', revision:1, plan_id:plan.agent_plan_id, base_head_revision:plan.head.head_revision, editor: {
            schema:'hiroute.plan-editor/v2',display_name:plan.desired.display_name,purpose:plan.desired.purpose,mode:'smart_saving',candidates:[],smart,
            branch_routing:custom,free:{candidates:[],primary:[],primary_fallback:false},delegation_enabled:false,requirements:plan.desired.requirements,limits:plan.desired.limits,
          } }];
        }
        return snapshot;
      };
      // Synthetic IPC models a server response, not persistence or product acceptance.
      control().handlers.plan_editor_transition_preview = ({ input }) => {
        if (same) return {};
        const editor = input.editor, branchMode = editor.mode === 'custom_branches';
        const settings = branchMode ? editor.branch_routing : editor.smart;
        const scopes = branchMode ? [...(settings.branches.some(b => !b.judgment) ? [{id:'default',judgment:settings.judgment}] : []), ...settings.branches.filter(b => b.judgment).map(b => ({id:b.id,judgment:b.judgment}))] : [{id:'smart_saving',judgment:settings.judgment}];
        return { assessment_transition: { scopes: scopes.map(({id,judgment}) => {
          const choice = input.assessment_choices?.[id];
          const effective = choice?.method === 'stock' && settings.native_protocol ? stockAssessment(judgment, settings.native_protocol) : judgment;
          const assessment = composedAssessment(settings.native_protocol, effective);
          assessment.instructions += ' Server Preview content for this publication.';
          return {scope:id,from_protocol:c?'hiroute.system-one-c/v1':'hiroute.system-one-legacy/v1',protocol:settings.native_protocol?'hiroute.system-one-c/v1':'hiroute.system-one-legacy/v1',assessment,assessment_digest:'sha256:'+'d'.repeat(64),available_methods:['stock','reviewed_custom',...(settings.native_protocol?[]:['historical'])],historical_sources:settings.native_protocol?[]:[{source:{...source,scope:id},instructions:'Retained assessment',criteria:['Failed','Partial','Complete']}],choice:choice??null};
        }) } };
      };
    });
    localStorage.setItem('hiroute.language', language); control().remount();
    if (language === 'zh') await routing();
    else { await click('Smart routing'); await until(() => all('.master-list .list-row').length, 'saved routes'); all('.master-list .list-row')[0].click(); }
    await until(() => document.querySelector('.native-protocol-editor select'), 'native behavior selector');
    await until(() => calls('plan_editor_options').some(c => c.payload.input.editor?.smart.classifier.kind === 'decision_service'), 'loaded model options');
  }
  function geometry() {
    for (const element of all('.native-protocol-editor .field-label, .native-prompt, .native-content-digest, .native-protocol-editor button')) {
      assert(element.scrollWidth <= element.clientWidth + 1, 'Assessment text or action overflows');
      if (element.classList.contains('field-label')) assert(element.getBoundingClientRect().width >= 160, 'Label collapsed under control pressure');
    }
  }
  return [scenario('desktop.routing.c-scope-review', ['routing-editor', 'decision-services', 'c-protocol'], 'Publication Preview confirms server-composed scope choices outside the editor', async () => {
    for (const language of ['zh','en']) {
      const t = (zh,en) => language === 'zh' ? zh : en;
      await open(language);
      const select = () => document.querySelector('.native-protocol-editor select');
      assert(select().value === 'legacy', 'Existing route automatically changed behavior');
      choose(select(), 'applicability');
      await until(() => text().includes(t('本次打开尚未测试历史问题适用性','Not tested in this session')), 'session evidence');
      assert(!all('[data-assessment-transition]').length, 'Editable selector stores a review');
      control().handlers.test_classifier_decision = async ({input}) => {
        const target = input.request.target;
        assert(target.kind === 'saved' && target.service_revision === 7 && target.shape === 'standard', 'Test lost saved revision/shape');
        const bytes = await crypto.subtle.digest('SHA-256',new TextEncoder().encode(JSON.stringify(service.connection.endpoint)));
        return {schema:'hiroute.classifier-decision-test-result/v2',state:'verified',verified_shapes:['degree','historical_assessment','applicability'],service_id:service.id,service_revision:7,model:service.connection.model,endpoint_digest:'sha256:'+ [...new Uint8Array(bytes)].map(b=>b.toString(16).padStart(2,'0')).join(''),protocol_digest:target.protocol.digest,shape:'standard',question_count:3,tested_at_unix_ms:Date.now(),duration_millis:1,reason:'complete'};
      };
      await click(t('测试此路由的适用性（1 次请求）','Test route applicability (1 request)'));
      await until(() => text().includes(t('适用性请求形状已验证','Applicability request shape verified')), 'verified exact test');
      setInput(document.querySelector('.native-protocol-editor input[type="number"]'),'0');
      await until(() => text().includes(t('背景为 0','With zero background')), 'zero background warning');
      assert(text().includes(t('本次打开尚未测试历史问题适用性','Not tested in this session')), 'Changed content reused evidence');
      setInput(document.querySelector('.native-protocol-editor input[type="number"]'),'8');
      await until(() => !text().includes(t('适用性请求形状已验证','Applicability request shape verified')), 'returning to old content does not resurrect evidence');
      setInput(document.querySelector('.native-protocol-editor input[type="number"]'),'0');
      await click(t('保存草稿','Save draft'));
      await until(() => calls('preview_plan_editor').length && !all('.plan-editor fieldset')[0].disabled, 'draft submitted');
      const saved = calls('preview_plan_editor').at(-1).payload.input.editor.smart;
      assert(saved.native_protocol.prior_context_max_turns === 0 && !('assessment_review' in saved), 'Draft persisted a review record');
      assert(saved.judgment.degree.simple_threshold_millis === 800 && saved.judgment.competence.floor_millis === 500, 'Selection changed thresholds');
      await click(t('发布更改','Publish changes'));
      await until(() => all('[data-assessment-transition]').length, 'server transition Preview');
      assert(!calls('preview_plan_editor').some(c=>c.payload.input.action==='publish'), 'Unconfirmed transition published');
      await click(t('采用目标方式的默认评分','Use destination default'));
      await until(() => text().includes(t('已选择，将随本次发布记录','Chosen for this publication')), 'choice refreshed Preview');
      assert(text().includes('Server Preview content for this publication.') && text().includes('sha256:'+'d'.repeat(64)), 'UI replaced server-composed content or digest');
      const choices = calls('plan_editor_transition_preview').at(-1).payload.input.assessment_choices;
      assert(JSON.stringify(choices) === JSON.stringify({smart_saving:{method:'stock'}}), 'Client supplied review hashing/provenance');
      geometry();
      await click(t('确认评分标准并发布','Confirm assessments and publish'));
      await until(() => calls('preview_plan_editor').some(c=>c.payload.input.action==='publish'), 'confirmed publication dispatch');
      const input = calls('preview_plan_editor').at(-1).payload.input;
      assert(!('assessment_review' in input.editor.smart) && input.assessment_choices.smart_saving.method === 'stock', 'Publication lost transient choice');
      assert(calls('test_classifier_decision').length === 1, 'Save/Preview/publication probed a provider');
      document.querySelector('[data-assessment-transition]').scrollIntoView({block:'start'});
    }
  }), scenario('desktop.routing.same-protocol-edit', ['routing-editor','decision-services','c-protocol'], 'Ordinary competence edits within either protocol carry no review state', async () => {
    for (const language of ['zh','en']) for(const c of [false,true]) for(const branches of [false,true]) {
      const t=(zh,en)=>language==='zh'?zh:en;
      await open(language,{c,branches,same:true});
      const field = document.querySelector(`[data-decision-field="${branches?'branch-0':'smart'}-competence-instructions"]`);
      assert(field,'Competence field missing');
      const parents=[]; for(let p=field.parentElement;p;p=p.parentElement) if(p.tagName==='DETAILS')parents.unshift(p);
      for(const p of parents) if(!p.open)p.querySelector('summary').click();
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value').set.call(field,field.value+' Assess the requested deliverable.'); field.dispatchEvent(new Event('input',{bubbles:true}));
      await click(t('发布更改','Publish changes'));
      await until(() => calls('preview_plan_editor').some(c=>c.payload.input.action==='publish'), 'same-protocol publication');
      const input = calls('preview_plan_editor').at(-1).payload.input;
      assert(!all('[data-assessment-transition]').length && Object.keys(input.assessment_choices).length === 0, 'Ordinary edit required protocol re-review');
      assert(!JSON.stringify(input.editor).includes('assessment_review'), 'Published provenance entered editor state');
      assert(!all('button').some(b=>/Remove this review record|移除此审阅记录/.test(b.textContent)), 'Obsolete review-removal action remains');
      assert(calls('test_classifier_decision').length===0,'Ordinary publication dispatched a model test');
    }
  }), scenario('desktop.routing.c-cross-mode-rollback', ['routing-editor','decision-services','c-protocol'], 'C smart to legacy branches requires a destination choice at publication', async () => {
    for(const language of ['zh','en']) {
      const t=(zh,en)=>language==='zh'?zh:en;
      await open(language,{c:true,parkedBranches:true});
      all('.mode-card').find(b=>b.querySelector('strong')?.textContent===t('自定义分支','Custom branches')).click();
      await until(()=>document.querySelector('.native-protocol-editor select')?.value==='legacy','legacy destination');
      await click(t('发布更改','Publish changes'));
      await until(()=>document.querySelector('[data-native-scope="default"]'),'cross-mode scope');
      assert(!calls('preview_plan_editor').length,'Cross-mode rollback skipped destination confirmation');
      assert(all('[data-native-scope]').length===1,'Inherited scopes were duplicated');
      await click(t('恢复版本 1 的评分','Restore revision 1 assessment'));
      await until(()=>text().includes(t('已选择，将随本次发布记录','Chosen for this publication')),'pinned historical choice');
      geometry();
      await click(t('确认评分标准并发布','Confirm assessments and publish'));
      await until(()=>calls('preview_plan_editor').length,'reviewed rollback dispatch');
      const input=calls('preview_plan_editor').at(-1).payload.input;
      assert(input.assessment_choices.default.method==='historical'&&input.assessment_choices.default.source.scope==='default','Historical choice is not pinned');
      assert(!input.editor.branch_routing.native_protocol&&!('assessment_review' in input.editor.branch_routing),'Rollback persisted editable review');
      assert(calls('test_classifier_decision').length===0,'Rollback implicitly called a provider');
      document.querySelector('[data-assessment-transition]').scrollIntoView({block:'start'});
    }
  }), scenario('desktop.decisions.diagnostic-shapes', ['decision-services','c-protocol'], 'One diagnostic action selects a shape and discards evidence after changing it', async () => {
    for(const language of ['zh','en']) {
      const t=(zh,en)=>language==='zh'?zh:en;
      await fresh(() => {
        control().handlers.decision_services=()=>({services:[service]});
        control().handlers.test_classifier_decision=async({input})=>{
          const target=input.request.target;
          assert(target.kind==='saved'&&target.service_revision===7,'Diagnostic lost exact saved revision');
          const bytes=await crypto.subtle.digest('SHA-256',new TextEncoder().encode(JSON.stringify(service.connection.endpoint)));
          return {schema:'hiroute.classifier-decision-test-result/v2',state:'verified',verified_shapes:['degree','historical_assessment','applicability'],service_id:service.id,service_revision:7,model:service.connection.model,endpoint_digest:'sha256:'+ [...new Uint8Array(bytes)].map(b=>b.toString(16).padStart(2,'0')).join(''),protocol_digest:target.protocol.digest,shape:target.shape,question_count:3,tested_at_unix_ms:Date.now(),duration_millis:1,reason:'complete'};
        };
      });
      localStorage.setItem('hiroute.language',language);control().remount();
      await click(t('模型','Models'));await click(t('决策模型','Decision models'));
      await until(()=>document.querySelector('.decision-test-actions select'),'diagnostic shape selector');
      const select=()=>document.querySelector('.decision-test-actions select');
      assert(all('button').filter(b=>[t('测试连接','Test connection'),t('测试所选形状','Test selected shape')].includes(b.textContent.trim())).length===1,'Parallel diagnostic actions remain');
      choose(select(),'standard');await click(t('测试所选形状','Test selected shape'));
      await until(()=>text().includes(t('适用性请求形状已验证','Applicability request shape verified')),'shape evidence');
      choose(select(),'max_branches');
      await until(()=>!text().includes(t('适用性请求形状已验证','Applicability request shape verified')),'shape change discards evidence');
      choose(select(),'standard');
      await until(()=>text().includes(t('本次打开尚未测试历史问题适用性','Not tested in this session')),'old shape cannot resurrect evidence');
      assert(calls('test_classifier_decision').length===1,'Changing shape implicitly tested');
      geometry();
    }
  }), scenario('desktop.quality.c-observations', ['routing-editor', 'sessions', 'c-protocol'], 'Retained C counts, denominators, partial coverage and empty states remain readable', async () => {
    for (const language of ['zh', 'en']) {
      const t = (zh, en) => language === 'zh' ? zh : en;
      let partial = false, empty = false;
      const cohort = { protocol: 'hiroute.system-one-c/v1', content_digest: 'sha256:' + 'a'.repeat(64), bundle_digest: 'sha256:' + 'b'.repeat(64),
        scope_id: 'synthetic-category-with-long-visible-name', standard: 'custom', provider: 'fixture-provider', model: 'synthetic-c-model-with-long-name', simple_threshold_millis: 800, competence_floor_millis: 500 };
      await fresh(() => {
        control().handlers.observation_read = payload => {
          if (payload.request.intent.view !== 'plan_quality') return control().fixtureResponse('observation_read', payload);
          assert(payload.request.schema === 'hiroute.observation.query/v3' && !('include_routing_observations' in payload.request.intent.query), 'Desktop did not use the current versioned query');
          return { samples: [], summary: { models: [], scored_stage_count: 0, unrated_stage_count: 0, session_count: 0, available_revisions: [8] }, next_cursor: null,
            routing_observations: { coverage: partial ? 'partial' : 'complete', covered_from_ms: 0, covered_to_ms: Date.now(), retained_decisions: empty ? 0 : 11,
              known_non_c_decisions: 1, legacy_or_unknown_decisions: partial ? 2 : 0, unknown_scope_decisions: 0, missing_projection_events: partial ? 1 : 0, scan_limited: partial,
              cohorts: empty ? [] : [{ cohort, counts: { c_decisions: 10, inherited: 1, requested: 7, unavailable: 4, no_target_omissions: 1, partial_target_omissions: 1,
                required_question_missing: 1, gate_opportunities: 4, suppressed: 1, unavailable_per_requested: partial ? null : 4 / 7,
                suppressed_per_opportunity: partial ? null : 1 / 4, suppressed_per_c_decision: partial ? null : 1 / 10 } }] } };
        };
      });
      localStorage.setItem('hiroute.language', language); control().remount();
      if (language === 'zh') await routing();
      else { await click('Smart routing'); await until(() => all('.master-list .list-row').length, 'saved routes'); all('.master-list .list-row')[0].click(); }
      await click(t('模型表现', 'Model performance'));
      await until(() => all('.routing-observations summary').length, 'C observation entry');
      all('.routing-observations summary')[0].click();
      await until(() => text().includes('57.1%') && text().includes('25.0%') && text().includes('10.0%'), 'retained denominator rates');
      assert(all('.routing-counts')[0].textContent.includes('4 / 7'), 'Unavailable numerator or request denominator lost');
      all('.routing-cohort details summary')[0].click();
      for (const element of all('.routing-counts dt, .routing-counts dd, .routing-cohort strong, .quality-diagnostic-identity')) {
        assert(element.scrollWidth <= element.clientWidth + 1, 'C observation identity or count overflows');
      }
      partial = true; all('.quality-refresh')[0].click();
      await until(() => text().includes(t('覆盖不完整，比例不可用', 'Partial coverage; rates unavailable')), 'partial coverage');
      assert(!all('.routing-counts')[0].textContent.includes('%') && all('.routing-counts')[0].textContent.includes('4 / 7'), 'Partial coverage hides counts or claims a rate');
      empty = true; partial = false; all('.quality-refresh')[0].click();
      await until(() => text().includes(t('此范围内暂无历史问题适用性决策记录', 'No historical problem applicability decisions are available')), 'empty retained scope');
    }
  })];
}
