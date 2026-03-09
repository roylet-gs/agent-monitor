const MOCK_API_URL = process.env.MOCK_API_URL || "http://localhost:4100";

interface MockSetupConfig {
  /** gh CLI response fixture. Set to null to simulate "no PR found". */
  gh?: unknown | null;
  /** Linear GraphQL response fixture. */
  linear?: unknown;
  /** gh --version response string. */
  ghVersion?: string;
}

/** Configure mock-api fixtures for the current test. */
export async function setupMock(config: MockSetupConfig): Promise<void> {
  const res = await fetch(`${MOCK_API_URL}/mock/setup`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(config),
  });
  if (!res.ok) {
    throw new Error(`Failed to setup mock: ${res.status} ${await res.text()}`);
  }
}

/** Reset mock-api fixtures back to defaults. */
export async function resetMock(): Promise<void> {
  const res = await fetch(`${MOCK_API_URL}/mock/reset`, {
    method: "DELETE",
  });
  if (!res.ok) {
    throw new Error(`Failed to reset mock: ${res.status} ${await res.text()}`);
  }
}
