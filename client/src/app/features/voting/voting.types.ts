export type QuestionType = 'single' | 'multiple' | 'numeric';

export interface Question {
  id: number;
  text: string;
  question_type: QuestionType;
  options: string[] | null;
}

export interface ParticipantAdminView {
  participant_id: string;
  approved: boolean;
  has_voted: boolean;
  enc_name_chunks?: string[];

  decrypted_name?: string | null;
}