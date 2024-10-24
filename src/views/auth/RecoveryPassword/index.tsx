import React from 'react'
import { ButtonAuthCommon } from '../../../styles/CommonsComponents'
import { RecoveryContent } from './styles'

function RecoveryPassword() {
  return (
    <>
      <RecoveryContent>
        <div className='main'>
          <h1>Esqueceu sua senha?</h1>
          <p>Escreva abaixo o email de recuperação</p>

          <div className='email'>
            <label htmlFor=''>Email:</label>
            <input type='text' id='email' autoComplete='new-password' />
          </div>

          <ButtonAuthCommon type='button' style={{ width: '100%' }}>
            Enviar
          </ButtonAuthCommon>
        </div>
      </RecoveryContent>
    </>
  )
}

export default RecoveryPassword
