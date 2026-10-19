import { useState } from 'react';
import { copyText } from '../../ui';

export function ClaudeLaunchCommand({ language }: { language: 'zh' | 'en' }) {
  const [copied, setCopied] = useState(false), [failed, setFailed] = useState(false);
  const t = (zh: string, en: string) => language === 'zh' ? zh : en;
  const command = 'hiroute agent launch --agent claude-code --';
  return <div className="worker-choices" data-claude-launch-command>
    <p className="field-help">{t('仅通过 HiRoute 启动的会话使用这些映射；普通 claude 保留原生登录和配置。请保持 HiRoute 运行。', 'Only sessions started through HiRoute use these mappings. Ordinary claude keeps native sign-in and settings. Keep HiRoute running.')}</p>
    <code style={{ overflowWrap: 'anywhere' }}>{command}</code>
    <div className="actions"><button type="button" className="btn" onClick={() => { void copyText(command).then(() => { setCopied(true); setFailed(false); }).catch(() => { setCopied(false); setFailed(true); }); }}>{t('复制启动命令', 'Copy launch command')}</button></div>
    {(copied || failed) && <span role="status" className="field-help">{failed ? t('复制失败，请手动复制上面的命令。', 'Copy failed. Copy the command above manually.') : t('已复制，粘贴到终端即可启动。', 'Copied. Paste into a terminal to start.')}</span>}
  </div>;
}
