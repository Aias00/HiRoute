import { scenario } from './scenario-selection.mjs';

export function cProtocolScenarios({ fresh, routing, click, until, all, text, calls, setInput, control }) {
  const assert = (value, message) => { if (!value) throw new Error(message); };
  const choose = (select, value) => { select.value = value; select.dispatchEvent(new Event('change', { bubbles: true })); };
  const service = { id: 'decision/c-ui-fixture', revision: 7, name: 'Saved C fixture', connection: {
    kind: 'system_one', provider: 'compatible', model: 'synthetic-c-model', endpoint: 'https://decision.example/systemone', timeout_ms: 3000,
    auth_header: { name: 'Authorization', value_secret_ref: 'decision/c-ui-fixture/r7' },
  } };
  return [scenario('desktop.routing.c-scope-review', ['routing-editor', 'decision-services', 'c-protocol'], 'C scope adoption, session evidence, rollback and long composed text remain usable', async () => {
    for (const language of ['zh', 'en']) {
      const t = (zh, en) => language === 'zh' ? zh : en;
      await fresh(() => { control().handlers.decision_services = () => ({ services: [service] }); });
      localStorage.setItem('hiroute.language', language); control().remount();
      if (language === 'zh') await routing();
      else {
        await click('Smart routing');
        await until(() => all('.master-list .list-row').length, 'saved routes'); all('.master-list .list-row')[0].click();
        await until(() => document.querySelector('.plan-identity-fields input'), 'English route editor');
      }
      await click(t('决策模型', 'Decision model'));
      await until(() => document.querySelector('.native-protocol-editor select'), 'native protocol selector');
      const select = () => document.querySelector('.native-protocol-editor select');
      assert(select().value === 'legacy', 'Existing route automatically adopted C');
      choose(select(), 'c');
      await until(() => all('[data-native-scope="smart_saving"]').length, 'effective C scope');
      assert(text().includes(t('本次打开尚未测试 C 能力', 'Not tested in this session')), 'Missing session evidence is not explicit');
      await click(t('采用标准 C 评分', 'Adopt stock C assessment'));
      const review = () => document.querySelector('[data-native-scope="smart_saving"] details');
      review().querySelector('summary').click();
      await until(() => review().querySelector('code')?.textContent.startsWith('sha256:'), 'composed review digest');
      assert(review().textContent.includes('state.assessment_stage') && review().querySelectorAll('.native-prompt').length === 4, 'Incomplete composed question or criteria');
      await click(t('已审阅，确认此范围', 'Reviewed: confirm this scope'));
      await until(() => text().includes(t('已审阅当前协议与完整评分标准', 'Current protocol and composed assessment reviewed')), 'scope review completed');
      assert(calls('test_classifier_decision').length === 0, 'Adoption implicitly tested a model');
      control().handlers.test_classifier_decision = async ({ input }) => {
        const request = input.request;
        assert(request.service_revision === 7 && request.shape === 'standard', 'Probe lost exact saved revision or shape');
        const bytes = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(JSON.stringify(service.connection.endpoint)));
        return { schema: 'hiroute.classifier-decision-test-result/v2', state: 'verified', service_id: service.id, service_revision: 7,
          model: service.connection.model, endpoint_digest: 'sha256:' + [...new Uint8Array(bytes)].map(b => b.toString(16).padStart(2, '0')).join(''),
          protocol_digest: request.protocol.digest, shape: 'standard', question_count: 3, tested_at_unix_ms: Date.now(), duration_millis: 1, reason: 'complete' };
      };
      await click(t('测试此路由的 C 能力（1 次请求）', 'Test this route’s C capability (1 request)'));
      await until(() => text().includes(t('C 能力已验证', 'C capability verified')), 'exact-session capability result');
      setInput(document.querySelector('.native-protocol-editor input[type="number"]'), '0');
      await until(() => text().includes(t('背景为 0', 'With zero background')), 'zero-background warning');
      assert(text().includes(t('本次打开尚未测试 C 能力', 'Not tested in this session')), 'Changed content reused capability evidence');
      await click(t('保存草稿', 'Save draft'));
      await until(() => calls('preview_plan_editor').length, 'C draft boundary');
      const saved = calls('preview_plan_editor').at(-1).payload.input.editor.smart;
      assert(saved.native_protocol.prior_context_max_turns === 0 && saved.assessment_review.smart_saving.protocol === 'hiroute.system-one-c/v1', 'C draft lost scope review');
      assert(saved.judgment.degree.simple_threshold_millis === 800 && saved.judgment.competence.floor_millis === 500, 'Adoption changed numeric thresholds');
      choose(select(), 'legacy');
      await click(t('采用旧默认评分', 'Adopt legacy default assessment'));
      if (!review().open) review().querySelector('summary').click();
      await click(t('已审阅，确认此范围', 'Reviewed: confirm this scope'));
      await until(() => !all('.operation-feedback').some(e => e.textContent.includes('Saving')), 'draft settles');
      await click(t('保存草稿', 'Save draft'));
      await until(() => calls('preview_plan_editor').at(-1).payload.input.editor.smart.assessment_review.smart_saving.protocol === 'hiroute.system-one-legacy/v1', 'destination-specific rollback review');
      assert(!calls('preview_plan_editor').at(-1).payload.input.editor.smart.native_protocol, 'Rollback retained C execution mode');
      assert(calls('test_classifier_decision').length === 1, 'Draft save or rollback dispatched another probe');
      // Leave the full C composition open for host screenshot inspection.
      choose(select(), 'c'); await click(t('采用标准 C 评分', 'Adopt stock C assessment'));
      if (!review().open) review().querySelector('summary').click();
      review().scrollIntoView({ block: 'center' });
      await until(() => review().querySelector('code')?.textContent.startsWith('sha256:'), 'visible review content');
      for (const element of all('.native-protocol-editor .field-label, .native-protocol-editor .native-prompt, .native-content-digest, .native-protocol-editor button')) {
        assert(element.scrollWidth <= element.clientWidth + 1, 'C text or action overflows horizontally');
        if (element.classList.contains('field-label')) assert(element.getBoundingClientRect().width >= 160, 'C label collapsed under control pressure');
      }
      const footer = document.querySelector('.plan-editor-footer, .editor-actions, .plan-editor .modal-foot');
      if (footer) assert(footer.getBoundingClientRect().bottom <= innerHeight + 1, 'Save actions left the supported viewport');
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
          assert(payload.request.intent.query.include_routing_observations === true, 'Desktop omitted the opt-in projection');
          return { samples: [], summary: { models: [], scored_stage_count: 0, unrated_stage_count: 0, session_count: 0, available_revisions: [8] }, next_cursor: null,
            routing_observations: { coverage: partial ? 'partial' : 'complete', covered_from_ms: 0, covered_to_ms: Date.now(), retained_decisions: empty ? 0 : 11,
              legacy_or_unknown_decisions: partial ? 2 : 0, unknown_scope_decisions: 0, missing_projection_events: partial ? 1 : 0, scan_limited: partial,
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
      await until(() => text().includes(t('此范围内暂无可用的 C 决策记录', 'No C decision records are available')), 'empty retained scope');
    }
  })];
}
