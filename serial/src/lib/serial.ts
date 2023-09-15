export async function read(
  port: SerialPort,
  signal: AbortSignal
): Promise<Uint8Array | undefined> {
  const reader = port.readable.getReader();
  function cancel() {
    reader.cancel(signal.reason);
  }

  signal.addEventListener("abort", cancel, { once: true });

  try {
    const { value, done } = await reader.read();

    if (done) {
      console.log("Done reading");
      return;
    }

    if (!value) {
      console.log("No value");
      return;
    }

    if (!(value instanceof Uint8Array))
      throw new Error("Expected value read to be binary");

    console.log("Read value: ", value);
    return value;
  } finally {
    signal.removeEventListener("abort", cancel);
    reader.releaseLock();
  }
}

export async function write(
  port: SerialPort,
  frame: Uint8Array | ArrayBuffer,
  signal: AbortSignal
) {
  const writer = port.writable.getWriter();
  function cancel() {
    writer.abort(signal.reason);
  }

  signal.addEventListener("abort", cancel, { once: true });

  try {
    await writer.write(frame);
  } finally {
    signal.removeEventListener("abort", cancel);
    writer.releaseLock();
  }
}
