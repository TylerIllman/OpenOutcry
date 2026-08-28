import type {
  CreateSessionRequest,
  CreateSessionResponse,
  JoinSessionRequest,
  JoinSessionResponse,
  SessionMeta,
} from "./protocol";

/**
 * HTTP surface. Four endpoints; everything else happens over the socket.
 *
 *   POST /api/sessions              -> create a session, returns code + hostToken
 *   GET  /api/sessions/:code        -> public metadata, for the join screen
 *   POST /api/sessions/:code/join   -> claim a seat, returns playerId + playerToken
 *   GET  /api/sessions/:code/export.csv?hostToken=...  -> tape + final book
 */

export class ApiError extends Error {
  constructor(public status: number, message: string) {
    super(message);
  }
}

async function post<TReq, TRes>(path: string, body: TReq): Promise<TRes> {
  const res = await fetch(path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!res.ok) throw new ApiError(res.status, await res.text());
  return (await res.json()) as TRes;
}

export async function createSession(
  req: CreateSessionRequest,
): Promise<CreateSessionResponse> {
  return post<CreateSessionRequest, CreateSessionResponse>("/api/sessions", req);
}

export async function getSession(code: string): Promise<SessionMeta> {
  const res = await fetch(`/api/sessions/${encodeURIComponent(code)}`);
  if (!res.ok) throw new ApiError(res.status, await res.text());
  return (await res.json()) as SessionMeta;
}

export async function joinSession(
  code: string,
  req: JoinSessionRequest,
): Promise<JoinSessionResponse> {
  return post<JoinSessionRequest, JoinSessionResponse>(
    `/api/sessions/${encodeURIComponent(code)}/join`,
    req,
  );
}

export function exportUrl(code: string, hostToken: string): string {
  return `/api/sessions/${encodeURIComponent(code)}/export.csv?hostToken=${encodeURIComponent(hostToken)}`;
}
