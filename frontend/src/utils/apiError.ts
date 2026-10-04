/**
 * Turn a thrown request failure into something a human can act on.
 *
 * Every handler in this API answers through one envelope, `{ success, error }`,
 * so reading `err.response.data.error` is right for anything the handler itself
 * refused. It is NOT right for a request the handler never saw: axum rejects a
 * body that fails to deserialize in its `Json` extractor, before any of our
 * code runs, and answers **422 with a plain-text body** describing the field.
 *
 * Callers that only read the envelope fall through to `err.message` there, which
 * is "Request failed with status code 422" -- a message that names neither the
 * field nor the problem, and sends whoever hit it to the server logs to find out
 * what they typed wrong. That is how a mismatched form field can sit unnoticed:
 * the one person positioned to report it has nothing to report.
 *
 * Order matters. The envelope is tried first because it is the specific case;
 * the plain-text body second; the generic axios message last.
 */
export function apiErrorMessage(err: unknown, fallback: string): string {
  const e = err as {
    response?: { status?: number; data?: unknown }
    message?: string
  }

  const data = e?.response?.data

  // 1. Our own envelope.
  if (data && typeof data === 'object') {
    const envelope = data as { error?: unknown; message?: unknown }
    if (typeof envelope.error === 'string' && envelope.error.trim() !== '') {
      return envelope.error
    }
    if (typeof envelope.message === 'string' && envelope.message.trim() !== '') {
      return envelope.message
    }
  }

  // 2. A plain-text rejection from the framework, which is where a
  //    deserialization failure lands. Prefixed with the status so it is obvious
  //    this is a malformed request rather than a refusal on the merits.
  if (typeof data === 'string' && data.trim() !== '') {
    const status = e?.response?.status
    const detail = data.trim()
    return status ? `${status}: ${detail}` : detail
  }

  // 3. Whatever the transport said.
  if (typeof e?.message === 'string' && e.message.trim() !== '') {
    return e.message
  }

  return fallback
}
