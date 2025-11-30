#!/usr/bin/env node
/**
 * End-to-end tests for the cloudflare-wrpc worker
 *
 * This script tests the wRPC communication between Durable Objects.
 * It expects the worker to be running on http://127.0.0.1:8787
 */

const BASE_URL = process.env.WORKER_URL || 'http://127.0.0.1:8787';

/**
 * Make an HTTP request and return the response
 */
async function request(path, options = {}) {
    const url = `${BASE_URL}${path}`;
    const response = await fetch(url, {
        ...options,
        headers: {
            'Content-Type': 'application/json',
            ...options.headers,
        },
    });
    return response;
}

/**
 * Test helper - asserts a condition and reports the result
 */
function assert(condition, message) {
    if (!condition) {
        throw new Error(`Assertion failed: ${message}`);
    }
}

/**
 * Test: Health check endpoint
 */
async function testHealth() {
    console.log('Testing health endpoint...');
    const response = await request('/health');
    assert(response.ok, `Health check failed with status ${response.status}`);
    const text = await response.text();
    assert(text === 'ok', `Health check returned unexpected response: ${text}`);
    console.log('✓ Health check passed');
}

/**
 * Test: Counter increment via direct HTTP
 */
async function testCounterDirect() {
    console.log('Testing direct counter access...');

    // Reset counter
    const resetResponse = await request('/counter/test-counter/reset', { method: 'POST' });
    assert(resetResponse.ok, `Counter reset failed with status ${resetResponse.status}`);

    // Get initial value
    const getResponse = await request('/counter/test-counter/value');
    assert(getResponse.ok, `Counter get failed with status ${getResponse.status}`);
    const initialValue = parseInt(await getResponse.text(), 10);
    assert(initialValue === 0, `Expected initial value 0, got ${initialValue}`);

    // Increment
    const incResponse = await request('/counter/test-counter/increment', { method: 'POST' });
    assert(incResponse.ok, `Counter increment failed with status ${incResponse.status}`);
    const afterInc = parseInt(await incResponse.text(), 10);
    assert(afterInc === 1, `Expected value 1 after increment, got ${afterInc}`);

    // Get value again
    const getResponse2 = await request('/counter/test-counter/value');
    const finalValue = parseInt(await getResponse2.text(), 10);
    assert(finalValue === 1, `Expected final value 1, got ${finalValue}`);

    console.log('✓ Direct counter access passed');
}

/**
 * Test: Counter operations via wRPC orchestration
 */
async function testOrchestration() {
    console.log('Testing wRPC orchestration...');

    // Reset multiple counters via orchestration
    const resetResponse = await request('/orchestrate', {
        method: 'POST',
        body: JSON.stringify({
            counters: ['counter-a', 'counter-b', 'counter-c'],
            operation: 'reset',
        }),
    });
    assert(resetResponse.ok, `Orchestration reset failed with status ${resetResponse.status}`);

    // Increment counters with different amounts
    const incResponse = await request('/orchestrate', {
        method: 'POST',
        body: JSON.stringify({
            counters: ['counter-a', 'counter-b', 'counter-c'],
            operation: 'increment',
            amount: 5,
        }),
    });
    assert(incResponse.ok, `Orchestration increment failed with status ${incResponse.status}`);
    const incResult = await incResponse.json();

    // Verify all counters were incremented
    assert(incResult.results.length === 3, `Expected 3 results, got ${incResult.results.length}`);
    for (const result of incResult.results) {
        assert(result.value === 5, `Expected counter value 5, got ${result.value} for ${result.name}`);
    }

    // Get all counter values
    const getResponse = await request('/orchestrate', {
        method: 'POST',
        body: JSON.stringify({
            counters: ['counter-a', 'counter-b', 'counter-c'],
            operation: 'get',
        }),
    });
    assert(getResponse.ok, `Orchestration get failed with status ${getResponse.status}`);
    const getResult = await getResponse.json();

    // Verify values
    for (const result of getResult.results) {
        assert(result.value === 5, `Expected counter value 5, got ${result.value} for ${result.name}`);
    }

    console.log('✓ wRPC orchestration passed');
}

/**
 * Test: Multiple increments to same counter via wRPC
 */
async function testMultipleIncrements() {
    console.log('Testing multiple increments via wRPC...');

    // Reset the counter
    await request('/orchestrate', {
        method: 'POST',
        body: JSON.stringify({
            counters: ['multi-inc-counter'],
            operation: 'reset',
        }),
    });

    // Increment 10 times
    for (let i = 0; i < 10; i++) {
        const response = await request('/orchestrate', {
            method: 'POST',
            body: JSON.stringify({
                counters: ['multi-inc-counter'],
                operation: 'increment',
                amount: 1,
            }),
        });
        assert(response.ok, `Increment ${i + 1} failed`);
    }

    // Verify final value
    const getResponse = await request('/orchestrate', {
        method: 'POST',
        body: JSON.stringify({
            counters: ['multi-inc-counter'],
            operation: 'get',
        }),
    });
    const result = await getResponse.json();
    assert(result.results[0].value === 10, `Expected value 10, got ${result.results[0].value}`);

    console.log('✓ Multiple increments passed');
}

/**
 * Run all tests
 */
async function main() {
    console.log('='.repeat(60));
    console.log('Running cloudflare-wrpc end-to-end tests');
    console.log(`Base URL: ${BASE_URL}`);
    console.log('='.repeat(60));
    console.log('');

    const tests = [
        testHealth,
        testCounterDirect,
        testOrchestration,
        testMultipleIncrements,
    ];

    let passed = 0;
    let failed = 0;

    for (const test of tests) {
        try {
            await test();
            passed++;
        } catch (error) {
            console.error(`✗ ${test.name} failed:`, error.message);
            failed++;
        }
    }

    console.log('');
    console.log('='.repeat(60));
    console.log(`Results: ${passed} passed, ${failed} failed`);
    console.log('='.repeat(60));

    if (failed > 0) {
        process.exit(1);
    }
}

// Wait for the worker to be ready before running tests
async function waitForWorker(maxAttempts = 30) {
    console.log('Waiting for worker to be ready...');

    for (let i = 0; i < maxAttempts; i++) {
        try {
            const response = await fetch(`${BASE_URL}/health`);
            if (response.ok) {
                console.log('Worker is ready!');
                return true;
            }
        } catch (e) {
            // Worker not ready yet
        }
        await new Promise(resolve => setTimeout(resolve, 1000));
    }

    throw new Error('Worker did not become ready in time');
}

// Main entry point
waitForWorker()
    .then(() => main())
    .catch((error) => {
        console.error('Test suite failed:', error.message);
        process.exit(1);
    });
