type RefreshTask = () => void | Promise<void>;

const STAGGER_MS = 250;
const RATE_LIMIT_PAUSE_MS = 60_000;

let tasks = new Set<RefreshTask>();
let intervalId: ReturnType<typeof setInterval> | null = null;
let running = false;
let pausedUntil = 0;

function sleep(ms: number): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, ms));
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
}

export function stopRefreshScheduler(): void {
	if (intervalId) {
		clearInterval(intervalId);
		intervalId = null;
	}
}

async function runAllTasks(): Promise<void> {
	if (running || isRefreshPaused()) return;

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
