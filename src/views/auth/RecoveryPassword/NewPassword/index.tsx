import React from 'react'
import { ButtonAuthCommon } from '../../../../styles/CommonsComponents'
import { NewContent } from './styles'

function NewPassword() {
  return (
    <NewContent>
      <div className='main'>
        <p>Digite sua nova senha</p>

        <div className='email'>
          <label htmlFor=''>Nova Senha:</label>
          <input type='text' id='email' autoComplete='new-password' />
        </div>
        <div className='email'>
          <label htmlFor=''>Confirme a senha:</label>
          <input type='text' id='email' autoComplete='new-password' />
        </div>

        <ButtonAuthCommon type='button' style={{ width: '100%' }}>
          Enviar
        </ButtonAuthCommon>
      </div>
    </NewContent>
  )
}

export default NewPassword
