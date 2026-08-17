export type RelayProtocol = "responses" | "chatCompletions" | "anthropic";

export type RelayMode = "official" | "mixedApi" | "pureApi" | "aggregate";

export type RelayModelMapping = {
  requestModel: string;
  alias: string;
  protocol: RelayProtocol;
  contextWindow: string;
};
