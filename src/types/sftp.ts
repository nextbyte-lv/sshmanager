export interface SftpEntry {
  name: string;
  is_dir: boolean;
  size: number | null;
  modified: number | null;
}
