export type Message = { id: number; at: number; room: string; sender: string; recipient: string | null; brief: string; text: string };
export type Arrival = { name: string; owner: string; harness: string; room: string };

export type Frame =
  | { t: "message"; message: Message }
  | { t: "enter"; member: Arrival }
  | { t: "leave"; name: string };

export interface Bridge {
  relay(frame: Frame): Promise<void>;
}

export const OFFICE_NAME = "main";
