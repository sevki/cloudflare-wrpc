/**
 * WebSocket test for wRPC over WebSocket
 *
 * Tests the websocket-pingpong example by:
 * 1. Connecting to the WebSocket endpoint
 * 2. Sending a wRPC ping request
 * 3. Verifying the pong response
 */

import { WebSocket } from "ws";

const WS_URL = "ws://127.0.0.1:8787/ws/testuser";

async function runTest() {
  console.log("Connecting to WebSocket at", WS_URL);

  return new Promise((resolve, reject) => {
    const ws = new WebSocket(WS_URL);
    let testPassed = false;

    const timeout = setTimeout(() => {
      console.error("Test timeout - no response received");
      ws.close();
      reject(new Error("Test timeout"));
    }, 10000);

    ws.on("open", () => {
      console.log("WebSocket connected");

      // Send a wRPC ping request
      const request = {
        type: "request",
        version: "0.1.0",
        payload: {
          instance: "pingpong",
          function: "ping",
          params: { message: "hello from test" },
          version: "0.1.0",
        },
      };

      console.log("Sending request:", JSON.stringify(request));
      ws.send(JSON.stringify(request));
    });

    ws.on("message", (data) => {
      const message = data.toString();
      console.log("Received:", message);

      try {
        const response = JSON.parse(message);

        // Validate response structure
        if (response.type !== "response") {
          throw new Error(`Expected type 'response', got '${response.type}'`);
        }

        if (!response.payload) {
          throw new Error("Missing payload in response");
        }

        const payload = response.payload;

        if (payload.status !== "ok") {
          throw new Error(`Expected status 'ok', got '${payload.status}'`);
        }

        // Validate the pong response data
        const pong = payload.data;

        if (!pong.message || !pong.message.includes("pong")) {
          throw new Error(`Expected pong message, got '${pong.message}'`);
        }

        if (!pong.from) {
          throw new Error("Missing 'from' field in pong response");
        }

        console.log("✓ Response type is 'response'");
        console.log("✓ Payload status is 'ok'");
        console.log(`✓ Pong message: ${pong.message}`);
        console.log(`✓ From: ${pong.from}`);
        console.log("");
        console.log("All WebSocket tests passed!");

        testPassed = true;
        clearTimeout(timeout);
        ws.close();
      } catch (err) {
        console.error("Test failed:", err.message);
        clearTimeout(timeout);
        ws.close();
        reject(err);
      }
    });

    ws.on("close", () => {
      console.log("WebSocket closed");
      if (testPassed) {
        resolve();
      }
    });

    ws.on("error", (err) => {
      console.error("WebSocket error:", err.message);
      clearTimeout(timeout);
      reject(err);
    });
  });
}

// Run the test
runTest()
  .then(() => {
    console.log("Test completed successfully");
    process.exit(0);
  })
  .catch((err) => {
    console.error("Test failed:", err.message);
    process.exit(1);
  });
