import { useId, useState } from 'react';
import { UiIcon } from '../../ui';
import type { HomeRead } from './types';
import { tokenShareHeadline, type TokenShare } from './token-share';
import { formatTokenCount } from '../usage-presentation';
import './token-share.css';

export function TokenShareCard({ read, language, onRetry }: { read: HomeRead<TokenShare>; language: 'zh' | 'en'; onRetry(): void }) {
  const t = (zh: string, en: string) => language === 'zh' ? zh : en;
  const [explain, setExplain] = useState(false);
  const id = useId();
  const share = read.status === 'ready' ? read.data : null;
  return <section className="surface token-share-card" data-token-share>
    <div className="token-share-heading"><h2>{t('今日订阅外 Token 占比', 'Outside subscription today')}</h2>
      <div className="token-share-explanation" onMouseEnter={() => setExplain(true)} onMouseLeave={() => setExplain(false)}>
        <button type="button" className="btn btn-quiet" aria-label={t('了解占比的统计口径', 'About this figure')} aria-describedby={explain ? id : undefined} aria-expanded={explain}
          onClick={() => setExplain(value => !value)} onFocus={() => setExplain(true)} onBlur={() => setExplain(false)}><UiIcon name="info" /></button>
        {explain && <div role="tooltip" id={id} className="token-share-tooltip">
          {t('付费 API 和免费来源的已知 Token，占订阅、付费和免费来源已知 Token 的比例。输入（不含缓存）＋缓存读取＋缓存写入＋输出；未知计费类别单独列出。跨模型只是近似比较，不代表额度、费用或节省。', 'Known tokens from paid APIs and free sources, divided by known tokens from subscription, paid and free sources. Basis: uncached input + cache reads + cache writes + output. Unknown billing classes stay separate. Approximate across models; not quota, cost or savings.')}
          {share && <p>{share.period.timezone} · {t('按本地日期统计，区间不含结束时刻。', 'Local calendar date; the end instant is excluded.')}</p>}
        </div>}
      </div>
    </div>
    <strong className={`token-share-headline${share?.headline.kind === 'share' ? ' percentage' : ''}`} data-token-share-headline>
      {share ? tokenShareHeadline(share, language) : read.status === 'loading' ? t('正在读取…', 'Reading…') : t('暂时无法读取', 'Temporarily unavailable')}
    </strong>
    {read.status === 'error' && <button type="button" className="btn" onClick={onRetry}>{t('重试', 'Retry')}</button>}
    {share && <>
      {share.unknown_usage_requests > 0 && <p className="field-help" data-token-share-unknown>{t(`部分用量未知：${share.unknown_usage_requests} 次上游请求未记录用量，未计入比例。`, `Some usage unknown: ${share.unknown_usage_requests} upstream requests have no recorded usage and are excluded.`)}</p>}
      {share.by_billing_class.unknown.tokens.total > 0 && <p className="field-help">{t(`另有 ${formatTokenCount(share.by_billing_class.unknown.tokens.total, language)} Token 的计费类别未知，未计入比例。`, `${formatTokenCount(share.by_billing_class.unknown.tokens.total, language)} tokens have an unknown billing class and are excluded.`)}</p>}
      {Object.values(share.by_group).some(group => group.turns > 0) && <p className="field-help">{t('Agent 轮次：', 'Agent turns: ')}{(['regular', 'primary', 'other'] as const).map((key, index) => `${[t('常规', 'Regular'), t('主力', 'Primary'), t('其他', 'Other')][index]} ${share.by_group[key].turns}`).join(' · ')}</p>}
      {share.retention_boundary_partial && <p className="field-help">{t('此时段部分明细已过保留期限；这里只统计仍保留的请求。', 'Some details in this period have expired. Only retained requests are counted here.')}</p>}
    </>}
  </section>;
}
