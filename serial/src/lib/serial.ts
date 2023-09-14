export async function read(port: SerialPort): Promise<Uint8Array | undefined> {
  const reader = port.readable.getReader();

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
    reader.releaseLock();
  }
}

export async function write(port: SerialPort, frame: Uint8Array | ArrayBuffer) {
  const writer = port.writable.getWriter();
  try {
    await writer.write(frame);
  } finally {
    writer.releaseLock();
  }
}
