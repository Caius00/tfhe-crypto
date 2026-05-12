// src/app/features/voting/voting.types.ts
export type QuestionType = 'bool' | 'single' | 'multiple' | 'numeric';

export interface Question {
  id: number;
  text: string;
  question_type: QuestionType;
  options: string[] | null;
  multiple?: boolean;
}
