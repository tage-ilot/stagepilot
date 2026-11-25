const FIB_DELAYS = [2, 3, 5, 8, 13, 21, 30];

export function fibonacciBackoff(pollNumber: number, maxSeconds = 30): number {
  const index = Math.min(pollNumber, FIB_DELAYS.length - 1);
  const seconds = Math.min(FIB_DELAYS[index]!, maxSeconds);
  return seconds * 1000;
}

export function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

