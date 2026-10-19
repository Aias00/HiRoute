// Presentation only: the storage owner supplies the recovery disposition.
export type StorageRecovery = {
  schema: 'hiroute.storage-startup-recovery/v1';
  cause: 'missing_package' | 'invalid_package' | 'insufficient_space' | 'access_denied' | 'locked' | 'missing_key' | 'unsupported_format' | 'migration_failed' | 'unknown_state';
  action: 'reinstall_original' | 'retry_current' | 'preserve_and_diagnose';
};
export function storageRecoveryText(value: unknown, language: 'zh' | 'en', platform: string) {
  const text = (zh: string, en: string) => language === 'zh' ? zh : en;
  const record = value && typeof value === 'object' ? value as Partial<StorageRecovery> : null;
  const trusted = record?.schema === 'hiroute.storage-startup-recovery/v1';
  const cause = trusted ? record.cause : undefined;
  const reasons: Record<string, string> = {
    missing_package: text('缺少配套旧版本的完整恢复包，升级已停止。', 'The upgrade stopped because the matching complete previous package is missing.'),
    invalid_package: text('旧版本恢复包或升级交接记录无法通过校验。', 'The previous recovery package or installation handoff could not be verified.'),
    insufficient_space: text('数据所在磁盘的恢复材料与启动空间不足。', 'The storage volume has insufficient space for recovery material and startup.'),
    access_denied: text('无法安全访问所需文件，请检查占用、权限与所有者。', 'Required files could not be accessed safely. Check file use, permissions and ownership.'),
    locked: text('另一个 HiRoute 实例正在使用这组数据。', 'Another HiRoute instance is using this data group.'),
    missing_key: text('原数据所需的密钥无法读取或校验。', 'A key required by the original data is unavailable or invalid.'),
    unsupported_format: text('这组数据的格式没有经过验证的升级路径。', 'This data format has no verified upgrade path.'),
    migration_failed: text('数据升级未完成，需要核对保留的恢复材料。', 'The storage upgrade did not complete. Check the preserved recovery material.'),
  };
  const reason = cause && reasons[cause] || text('无法确认这组数据的升级状态。', 'The storage upgrade state could not be established.');
  let instructions = text('请保留所有数据版本、密钥、完整安装包和升级记录。不要直接降级、删除数据或反复重试。查看启动诊断和对应批次的完整恢复说明；整组还原前，先另存新版数据及其配套程序。缺少密钥或完整备份时，请保留材料并寻求支持。', 'Keep every data generation, key, complete package and upgrade journal. Do not downgrade, delete data or retry blindly. Inspect startup diagnostics and the matching complete-group recovery guide. Preserve newer data and its matching program separately before restoration. Missing keys or complete backups may require support.');
  const original = trusted && cause === 'missing_package' && record.action === 'reinstall_original';
  if (original) {
    instructions = platform === 'macos'
      ? text('本次升级尚未改变原数据组。请完全退出 HiRoute，重新安装升级前使用的完整原版 App，保留数据目录。打开原版后，通过“设置 → 版本升级”按受支持的升级顺序更新。', 'This upgrade has not changed the original data group. Fully quit HiRoute and reinstall the exact complete App used before this upgrade, keeping the data directory. Open that version and follow the supported sequence in Settings → Updates.')
      : text('原数据组仍保留。请停止后台服务，按安装指南恢复升级前的完整原版程序，保留数据。随后按受支持的安装流程提供配套旧包再升级；不要只替换单个程序或数据库文件。', 'The original data group was retained. Stop the background service and follow the installation guide to restore the exact complete original program, keeping the data. Then supply the matching old package through the supported upgrade workflow. Do not replace individual binaries or databases.');
  } else if (cause === 'insufficient_space') {
    instructions = text('请释放数据所在磁盘上的无关文件，保留原数据、封存历史和恢复包。空间足够后，重新打开当前版本继续恢复；不要删除升级备份。', 'Free unrelated files on the storage volume, keeping original data, sealed history and recovery packages. Once enough space is available, reopen the current version to continue recovery. Do not delete upgrade backups.');
  } else if (cause === 'access_denied' || cause === 'locked') {
    instructions = text('请关闭其他 HiRoute 实例并解决相关文件占用或访问问题，再重新打开当前版本。保留所有数据和升级记录，不要批量修改权限或清空目录。', 'Close other HiRoute instances and resolve the file-use or access issue, then reopen the current version. Keep all data and upgrade journals. Do not change permissions recursively or empty directories.');
  }
  return { reason, instructions, canRetry: trusted && record.action === 'retry_current' && ['insufficient_space', 'access_denied', 'locked'].includes(cause ?? '') };
}
