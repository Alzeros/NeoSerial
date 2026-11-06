export async function withSettingsTimeout<Value>(
  operation: Promise<Value>,
  stage: string,
  timeoutMs = 5000,
): Promise<Value> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      operation,
      new Promise<never>((_resolve, reject) => {
        timer = setTimeout(() => reject(new Error(`${stage}超时，请重试`)), timeoutMs);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}
