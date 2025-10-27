//! Simple deadlock detection helpers for mutexes and semaphores

use crate::task::ProcessControlBlock;
use alloc::vec;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;

/// Error code to return when a deadlock would occur
pub const DEADLOCK_ERR: isize = -(0xDEAD as isize);

/// Check if locking the given mutex would cause a deadlock for the current process
pub fn mutex_would_deadlock(
	proc: &Arc<ProcessControlBlock>,
	mutex_id: usize,
	current_tid: usize,
) -> bool {
	let inner = proc.inner_exclusive_access();
	// Build wait-for edges: waiter -> owner
	let mut edges: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
	for m_opt in inner.mutex_list.iter() {
		if let Some(m) = m_opt {
			if let Some(owner) = m.owner_tid() {
				for w in m.waiters().into_iter() {
					edges.entry(w).or_default().push(owner);
				}
			}
		}
	}
	// Candidate edge: current -> owner of target mutex if locked by someone else
	if let Some(m) = &inner.mutex_list[mutex_id] {
		if let Some(owner) = m.owner_tid() {
			if owner != current_tid {
				edges.entry(current_tid).or_default().push(owner);
			}
		}
	}
	drop(inner);
	// DFS from current_tid to detect a cycle back to current_tid
	fn dfs(u: usize, target: usize, edges: &BTreeMap<usize, Vec<usize>>, vis: &mut BTreeMap<usize, u8>) -> bool {
		// 0: unvisited, 1: visiting, 2: visited
		vis.insert(u, 1);
		if let Some(neis) = edges.get(&u) {
			for &v in neis.iter() {
				if v == target { return true; }
				match vis.get(&v).copied().unwrap_or(0) {
					0 => { if dfs(v, target, edges, vis) { return true; } },
					1 => { /* cycle elsewhere */ },
					_ => {}
				}
			}
		}
		vis.insert(u, 2);
		false
	}
	let mut vis: BTreeMap<usize, u8> = BTreeMap::new();
	dfs(current_tid, current_tid, &edges, &mut vis)
}

/// Check if down-ing the given semaphore would cause a deadlock
pub fn semaphore_would_deadlock(
	proc: &Arc<ProcessControlBlock>,
	sem_id: usize,
	current_tid: usize,
) -> bool {
	let inner = proc.inner_exclusive_access();
	let sem = Arc::clone(inner.semaphore_list[sem_id].as_ref().unwrap());
	drop(inner);
	let sin = sem.inner.exclusive_access();
	// If a down would not block, it's safe
	if sin.count > 0 {
		return false;
	}
	// Participants: holders, waiters, and current
	let mut tids: Vec<usize> = Vec::new();
	for (tid, cnt) in sin.allocations.iter() {
		if *cnt > 0 { tids.push(*tid); }
	}
	for tcb in sin.wait_queue.iter() {
		let tid = tcb
			.inner_exclusive_access()
			.res
			.as_ref()
			.unwrap()
			.tid;
		tids.push(tid);
	}
	if !tids.contains(&current_tid) { tids.push(current_tid); }
	// Map tid -> idx
	let mut idx: BTreeMap<usize, usize> = BTreeMap::new();
	for (i, &tid) in tids.iter().enumerate() { idx.insert(tid, i); }
	let n = tids.len();
	// Available
	let mut work = if sin.count > 0 { sin.count as usize } else { 0 };
	// Allocation[i]
	let mut alloc = vec![0usize; n];
	for (tid, cnt) in sin.allocations.iter() {
		if let Some(&i) = idx.get(tid) { alloc[i] = *cnt; }
	}
	// Need[i]: assume 1 for threads waiting on this semaphore and current, else 0
	let mut need = vec![0usize; n];
	for tcb in sin.wait_queue.iter() {
		let tid = tcb
			.inner_exclusive_access()
			.res
			.as_ref()
			.unwrap()
			.tid;
		if let Some(&i) = idx.get(&tid) { need[i] = 1; }
	}
	if let Some(&i) = idx.get(&current_tid) { need[i] = 1; }
	drop(sin);
	// Safety check
	let mut finish = vec![false; n];
	loop {
		let mut progressed = false;
		for i in 0..n {
			if !finish[i] && need[i] <= work {
				work += alloc[i];
				finish[i] = true;
				progressed = true;
			}
		}
		if !progressed { break; }
	}
	// if any unfinished, unsafe
	finish.iter().any(|&f| !f)
}
