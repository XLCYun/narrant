/** 从 typed command 错误或普通异常中提取用户可见详情。 */
export function errorDetail(error: unknown, fallback: string) {
  if (error instanceof Error) return error.message;
  if (typeof error === "string" && error.trim().length > 0) return error;
  if (typeof error === "object" && error && "detail" in error && typeof error.detail === "string") return error.detail;
  return fallback;
}
