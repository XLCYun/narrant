use super::*;

/// 写入与 Win32 API 同布局的响应，仅模拟查询边界，不替代进程回收。
fn snapshot(buffer: &mut [usize], assigned: u32, pids: &[usize]) {
    const HEADER: usize =
        offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList) / size_of::<usize>();
    // SAFETY: 查询器提供已对齐、含固定头部和至少一个 PID 的缓冲区。
    let info = unsafe {
        &mut *buffer
            .as_mut_ptr()
            .cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>()
    };
    info.NumberOfAssignedProcesses = assigned;
    info.NumberOfProcessIdsInList = pids.len() as u32;
    buffer[HEADER..HEADER + pids.len()].copy_from_slice(pids);
}

/// 回放 Windows 退出窗口实际观测到的成功响应，不应因计数滞后反复扩容到上限。
#[test]
fn successful_exit_snapshot_uses_returned_ids_without_retry() {
    for (assigned, pids) in [(1, vec![]), (2, vec![123])] {
        let mut calls = 0;
        let result = query_process_ids(Instant::now() + Duration::from_secs(2), |buffer| {
            calls += 1;
            snapshot(buffer, assigned, &pids);
            Ok(())
        })
        .unwrap();
        assert_eq!(
            result,
            pids.iter().map(|pid| *pid as u32).collect::<Vec<_>>()
        );
        assert_eq!(calls, 1, "成功响应不应重新查询或扩容");
    }
}

/// 真正的 ERROR_MORE_DATA 仍需扩容并重新取得完整快照。
#[test]
fn insufficient_buffer_retries_and_keeps_all_ids() {
    let pids = (1..=70).collect::<Vec<usize>>();
    let mut calls = 0;
    let result = query_process_ids(Instant::now() + Duration::from_secs(2), |buffer| {
        calls += 1;
        if calls == 1 {
            snapshot(buffer, 70, &pids[..64]);
            Err(io::Error::from_raw_os_error(ERROR_MORE_DATA as i32))
        } else {
            snapshot(buffer, 70, &pids);
            Ok(())
        }
    })
    .unwrap();
    assert_eq!(result, (1..=70).collect::<Vec<u32>>());
    assert_eq!(calls, 2);
}

/// 查询失败原样传播，不能将无效句柄误当成空间不足。
#[test]
fn query_error_is_not_retried() {
    let mut calls = 0;
    let error = query_process_ids(Instant::now() + Duration::from_secs(2), |_| {
        calls += 1;
        Err(io::Error::from_raw_os_error(ERROR_INVALID_PARAMETER as i32))
    })
    .unwrap_err();
    assert_eq!(error.raw_os_error(), Some(ERROR_INVALID_PARAMETER as i32));
    assert_eq!(calls, 1);
}

/// 成功标志也不能允许越界读取 PID 列表。
#[test]
fn returned_count_must_fit_buffer() {
    let error = query_process_ids(Instant::now() + Duration::from_secs(2), |buffer| {
        snapshot(buffer, 65, &[]);
        // SAFETY: 只写固定头部，故意模拟不一致的列表长度，不写越界 PID。
        unsafe {
            (*buffer
                .as_mut_ptr()
                .cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>())
            .NumberOfProcessIdsInList = 65;
        }
        Ok(())
    })
    .unwrap_err();
    assert!(error.to_string().contains("成员列表长度无效"));
}
