import assert from 'node:assert/strict';
import test from 'node:test';
import { storageRecoveryText } from '../src/storage-recovery.ts';
const report=(cause,action)=>({schema:'hiroute.storage-startup-recovery/v1',cause,action});
test('only the authoritative original-compatible missing-package result permits simple reinstall',()=>{
  for (const language of ['zh','en']) {
    const safe=storageRecoveryText(report('missing_package','reinstall_original'),language,'macos');
    assert.match(safe.instructions,language==='zh'?/尚未改变原数据组/:/has not changed the original data group/);
    for (const value of [undefined,report('missing_package','preserve_and_diagnose'),report('missing_key','reinstall_original'),report('future_cause','reinstall_original'),{...report('missing_package','reinstall_original'),schema:'future'}]) {
      const unknown=storageRecoveryText(value,language,'macos');
      assert.doesNotMatch(unknown.instructions,language==='zh'?/尚未改变原数据组/:/has not changed the original data group/);
      assert.equal(unknown.canRetry,false);
    }
  }
});
test('Windows does not instruct users to reinstall a macOS App and low space preserves backups',()=>{
  const windows=storageRecoveryText(report('missing_package','reinstall_original'),'en','windows');
  assert.doesNotMatch(windows.instructions,/App|Settings/);
  const low=storageRecoveryText(report('insufficient_space','retry_current'),'en','windows');
  assert.equal(low.canRetry,true);
  assert.match(low.instructions,/Do not delete upgrade backups/);
});
