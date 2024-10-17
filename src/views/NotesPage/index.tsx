import React from 'react'
import { NotesContent } from './styles'

function NotesPage() {
  return (
    <>
      <NotesContent>
        <div className='main'>
          <div className='header'>
            <h1>Minhas Notas</h1>

            <button type='button'>Nova Anotação</button>
          </div>
        </div>
      </NotesContent>
    </>
  )
}

export default NotesPage
