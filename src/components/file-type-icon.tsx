import { fileTypeIconUrl, resolveFileTypeIcon, type FileTypeIconKind, type FileTypeIconTheme } from "@/lib/file-type-icons";

/** File Type Icon 的展示输入。 */
type FileTypeIconProps = {
  path: string;
  kind: FileTypeIconKind;
  expanded?: boolean;
  theme: FileTypeIconTheme;
  testId?: string;
};

/** 呈现仅辅助识别文件类型、不会重复朗读路径的本地 SVG。 */
export function FileTypeIcon({ path, kind, expanded, theme, testId }: FileTypeIconProps) {
  const icon = resolveFileTypeIcon({ path, kind, expanded, theme });
  return <img data-testid={testId} src={fileTypeIconUrl(icon)} alt="" aria-hidden className="size-4 shrink-0 object-contain" />;
}
