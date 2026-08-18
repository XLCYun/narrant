/** 可由所属 lifecycle 恢复的应用错误作用域。 */
export type ApplicationErrorScope = "github-identity" | "pull-request-discovery";

/** 应用错误对应的用户可恢复操作。 */
export type ApplicationErrorOperation =
  | "load-status"
  | "begin-authorization"
  | "complete-authorization"
  | "sign-out"
  | "discover";

/** 各 lifecycle 共享的错误分类与呈现输入。 */
export type ApplicationError = {
  scope: ApplicationErrorScope;
  operation: ApplicationErrorOperation;
  detail: string;
};

/** 将未知失败归一为保留作用域和恢复操作的错误。 */
export function applicationError(
  scope: ApplicationErrorScope,
  operation: ApplicationErrorOperation,
  reason: unknown,
): ApplicationError {
  return { scope, operation, detail: String(reason) };
}
