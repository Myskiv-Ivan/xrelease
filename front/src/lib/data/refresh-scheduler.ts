type RefreshTask = () => void | Promise<void>;

const STAGGER_MS = 250;
const RATE_LIMIT_PAUSE_MS = 60_000;

let tasks = new Set<RefreshTask>();
let intervalId: ReturnType<typeof setInterval> | null = null;
let running = false;
let pausedUntil = 0;
/** A tick was skipped because the tab was hidden — catch up on return. */
let missedWhileHidden = false;
let detachVisibility: (() => void) | null = null;

function sleep(ms: number): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * A background tab is not being read, so polling it only burns the operator's
 * battery and the instance's rate limit. The dashboard stays on screen for
 * hours, so this is the difference between a few refreshes and hundreds.
 */
function isHidden(): boolean {
	return typeof document !== 'undefined' && document.visibilityState === 'hidden';
}

export function isRefreshPaused(): boolean {
	return Date.now() < pausedUntil;
}

export function pauseRefresh(ms = RATE_LIMIT_PAUSE_MS): void {
	pausedUntil = Date.now() + ms;
}

export function registerRefreshTask(task: RefreshTask): () => void {
	tasks.add(task);
	return () => {
		tasks.delete(task);
	};
}

export function startRefreshScheduler(intervalMs: number): void {
	stopRefreshScheduler();
	if (intervalMs <= 0) return;

	intervalId = setInterval(() => {
		void runAllTasks();
	}, intervalMs);

	if (typeof document === 'undefined') return;
	// Returning to a stale tab should show fresh data immediately rather than
	// after up to one more full interval.
	const onVisibilityChange = () => {
		if (isHidden() || !missedWhileHidden) return;
		missedWhileHidden = false;
		void runAllTasks();
	};
	document.addEventListener('visibilitychange', onVisibilityChange);
	detachVisibility = () => document.removeEventListener('visibilitychange', onVisibilityChange);
}

export function stopRefreshScheduler(): void {
	if (intervalId) {
		clearInterval(intervalId);
		intervalId = null;
	}
	detachVisibility?.();
	detachVisibility = null;
	missedWhileHidden = false;
}

async function runAllTasks(): Promise<void> {
	if (running || isRefreshPaused()) return;
	if (isHidden()) {
		missedWhileHidden = true;
		return;
	}

	running = true;
	try {
		for (const task of tasks) {
			await task();
			await sleep(STAGGER_MS);
		}
	} finally {
		running = false;
	}
}
