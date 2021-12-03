import {IpcClient} from "./ipc-client";
import {FileItem, FileSession} from "../../../types";
import {Attributes, FileEntry} from "ssh2-streams";
import path from "path";

export class IpcFileSession extends IpcClient implements FileSession {
  constructor(private token: string) {
    super('file-session');
  }

  downloadTemp(remotePath: string): Promise<string> {
    return this.call('downloadTemp', this.token, remotePath);
  }

  get(remotePath: string, localPath: string): Promise<void> {
    return this.call('get', this.token, remotePath, localPath);
  }

  put(localPath: string, remotePath: string): Promise<void> {
    return this.call('put', this.token, localPath, remotePath);
  }

  readdir(location: string): Promise<FileEntry[]> {
    return this.call('readdir', this.token, location);
  }

  readdir_ext(location: string): Promise<FileItem[]> {
    return this.call('readdir_ext', this.token, location);
  }

  readlink(path: string): Promise<string> {
    return this.call('readlink', this.token, path);
  }

  rm(path: string, recursive: boolean): Promise<void> {
    return this.call('rm', this.token, path, recursive);
  }

  stat(path: string): Promise<Attributes> {
    return this.call('stat', this.token, path);
  }

  end(): Promise<void> {
    return this.call('close', this.token);
  }

  async uploadBatch(sources: string[], destination: string, error?: (name: string, error: Error) => Promise<boolean>): Promise<void> {
    for (const source of sources) {
      const filename: string = path.parse(source).base;
      let result = false;
      do {
        try {
          await this.put(source, path.posix.join(destination, filename));
        } catch (e) {
          if (!error) throw e;
          result = await error.call(filename, e);
        }
      } while (result);
      if (result === null) {
        break;
      }
    }
  }

}
