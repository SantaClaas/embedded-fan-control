import { describe, expect, it } from "vitest";
import { append } from "./crc";
import { bytes, relay, temperatureSensor } from "./frames.fixture";
import { readInputRegisters } from "./pdu";
import { BusMonitor, type MonitorEvent, type Observed } from "./monitor";

/** Joins frames into one buffer, the way they arrive when the adapter batches a burst */
function run(chunks: Uint8Array[], options?: { gapMs?: number; at?: (index: number) => number }) {
  const monitor = new BusMonitor(options);
  const events: MonitorEvent[] = [];

  chunks.forEach((chunk, index) => {
    events.push(...monitor.push(chunk, options?.at?.(index) ?? index));
  });

  return events;
}

function frames(events: MonitorEvent[]): Observed[] {
  return events.filter((event): event is MonitorEvent & Observed => event.type === "frame");
}

describe("BusMonitor", () => {
  it("recovers a request and its response from one chunk", () => {
    const events = run([concat(relay.readBaudRequest, relay.readBaudResponse)]);
    const seen = frames(events);

    expect(seen).toHaveLength(2);
    expect(seen[0]).toMatchObject({ role: "request", address: 0xff, functionCode: 0x03 });
    expect(seen[1]).toMatchObject({ role: "response", address: 0xff, functionCode: 0x03 });
    expect(seen[1]!.decoded).toEqual({ kind: "registers", of: "holdingRegisters", values: [4] });
  });

  it("recovers a frame split across chunks", () => {
    const whole = concat(relay.readBaudRequest, relay.readBaudResponse);

    // Split in the middle of the request, then again in the middle of the response
    const events = run([whole.subarray(0, 3), whole.subarray(3, 11), whole.subarray(11)]);

    expect(frames(events)).toHaveLength(2);
  });

  it("recovers frames arriving one byte at a time", () => {
    const whole = concat(relay.readBaudRequest, relay.readBaudResponse);
    const events = run(Array.from(whole, (byte) => Uint8Array.of(byte)));

    expect(frames(events)).toHaveLength(2);
  });

  /**
   * The reason the monitor tracks whose turn it is. A read request and the echo of a single write
   * are both eight bytes, so length alone cannot tell them apart — only the alternation can
   */
  it("does not mistake a request for a response of the same length", () => {
    const events = run([concat(relay.closeRelayRequest, relay.closeRelayResponse)]);
    const seen = frames(events);

    expect(seen.map((frame) => frame.role)).toEqual(["request", "response"]);
  });

  /**
   * A read response carries values but never the addresses they came from. Pairing is the only
   * way the UI can say which register a number belongs to
   */
  it("pairs a response with the request it answers", () => {
    const events = run([concat(temperatureSensor.readBothRequest, temperatureSensor.readBothResponse)]);
    const seen = frames(events);

    expect(seen[1]!.inReplyTo?.decoded).toEqual({
      kind: "read",
      of: "inputRegisters",
      start: 0x0001,
      quantity: 2,
    });
  });

  it("does not pair a response from a different device", () => {
    const events = run([concat(temperatureSensor.readBothRequest, relay.readBaudResponse)]);
    const seen = frames(events);

    expect(seen[1]!.inReplyTo).toBeUndefined();
  });

  /**
   * Joining a live bus lands in the middle of a frame, which is the normal case rather than the
   * exceptional one: the app connects whenever the user clicks, not between frames
   */
  it("resynchronises after joining mid-frame", () => {
    const whole = concat(relay.readBaudRequest, relay.readBaudResponse);
    // Start three bytes into the request, so the first frame is unrecoverable
    const events = run([whole.subarray(3)]);
    const seen = frames(events);

    expect(seen).toHaveLength(1);
    expect(seen[0]).toMatchObject({ role: "response", functionCode: 0x03 });
  });

  /**
   * The guard on the scan-ahead. A long read response arrives over several chunks, and while it is
   * in flight the *request* interpretation of its head is both long enough to check and wrong. If
   * that alone were taken as grounds to go looking further along, a legitimate frame would be cut
   * apart on nothing more than a coincidental checksum
   */
  it("does not cut apart a long response that is still arriving", () => {
    // 20 input registers, the shape of a RadiCal status poll
    const values = Array.from({ length: 20 }, (_, index) => 0x1000 + index);
    const response = registerResponse(0x02, values);
    const monitor = new BusMonitor();

    monitor.push(readRequest(0x02, 0xd010, values.length), 0);

    // Delivered a few bytes at a time, well inside the gap
    for (let offset = 0; offset < response.length; offset += 5) {
      monitor.push(response.subarray(offset, offset + 5), 1 + offset);
    }

    const events = monitor.push(new Uint8Array(0), 100);
    expect(events.filter((event) => event.type === "noise")).toHaveLength(0);

    // Re-run in one piece to confirm the frame itself is what we expected all along
    const whole = frames(run([concat(readRequest(0x02, 0xd010, values.length), response)]));
    expect(whole).toHaveLength(2);
    expect(whole[1]!.decoded).toMatchObject({ kind: "registers", values });
  });

  it("reports the bytes it could not account for instead of dropping them", () => {
    const events = run([concat(bytes("DE AD BE EF"), relay.readAddressRequest)]);

    const noise = events.filter((event) => event.type === "noise");
    expect(noise).toHaveLength(1);
    expect(frames(events)).toHaveLength(1);
    // Reported before the frame that followed it, so the order matches the wire
    expect(events[0]!.type).toBe("noise");
  });

  it("abandons a partial frame after a silence rather than letting it swallow the next one", () => {
    const monitor = new BusMonitor({ gapMs: 10 });

    // Half a request, then nothing for long enough that it can never complete
    expect(frames(monitor.push(relay.readBaudRequest.subarray(0, 4), 0))).toHaveLength(0);

    const after = monitor.push(relay.readAddressRequest, 100);

    expect(frames(after)).toHaveLength(1);
    expect(frames(after)[0]).toMatchObject({ address: 0x00, functionCode: 0x03 });
  });

  it("keeps a frame that merely straddles a chunk boundary within the gap", () => {
    const monitor = new BusMonitor({ gapMs: 50 });

    monitor.push(relay.readBaudRequest.subarray(0, 4), 0);
    const after = monitor.push(relay.readBaudRequest.subarray(4), 5);

    expect(frames(after)).toHaveLength(1);
  });

  it("decodes a whole conversation with several devices", () => {
    const events = run([
      concat(
        relay.readRelayStateRequest,
        relay.readRelayStateResponse,
        temperatureSensor.readBothRequest,
        temperatureSensor.readBothResponse,
        relay.closeRelayRequest,
        relay.closeRelayResponse,
      ),
    ]);
    const seen = frames(events);

    expect(seen).toHaveLength(6);
    expect(seen.map((frame) => frame.role)).toEqual([
      "request",
      "response",
      "request",
      "response",
      "request",
      "response",
    ]);
    expect(seen.map((frame) => frame.address)).toEqual([0xff, 0xff, 0x01, 0x01, 0xff, 0xff]);
  });

  it("hands back what is left when the port closes", () => {
    const monitor = new BusMonitor();
    monitor.push(relay.readBaudRequest.subarray(0, 4), 0);

    const remaining = monitor.flush(1);

    expect(remaining).toHaveLength(1);
    expect(remaining[0]).toMatchObject({ type: "noise" });
  });

  it("copies frames out of the working buffer", () => {
    const source = concat(relay.readAddressRequest, relay.readAddressResponse);
    const seen = frames(run([source]));

    // Scribbling on the source must not change what was already reported
    source.fill(0);

    expect(Array.from(seen[0]!.bytes)).toEqual(Array.from(relay.readAddressRequest));
  });
});

/** A read-input-registers response carrying `values`, built the way a device would */
function registerResponse(address: number, values: readonly number[]): Uint8Array {
  const frame = new Uint8Array(5 + values.length * 2);
  const view = new DataView(frame.buffer);

  frame[0] = address;
  frame[1] = 0x04;
  frame[2] = values.length * 2;
  values.forEach((value, index) => view.setUint16(3 + index * 2, value));
  append(frame);

  return frame;
}

function readRequest(address: number, start: number, quantity: number): Uint8Array {
  return readInputRegisters(address, start, quantity);
}

function concat(...parts: Uint8Array[]): Uint8Array {
  const joined = new Uint8Array(parts.reduce((total, part) => total + part.length, 0));
  let offset = 0;

  for (const part of parts) {
    joined.set(part, offset);
    offset += part.length;
  }

  return joined;
}
