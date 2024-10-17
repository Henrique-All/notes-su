import React, { useState, useRef } from 'react'
import Logo from '../../assets/Notebook.svg'
import Notes from '../../assets/notas.svg'
import Book from '../../assets/book.svg'
import Shared from '../../assets/comunidade.svg'
import User from '../../assets/user.svg'
import { Link } from 'react-router-dom'
import { Container, NavButton, NavLeft } from './styles'
import NotesPage from '../../views/NotesPage'

function NavBar() {
  const [page, setPage] = useState<number>(0)
  const buttonRefs = useRef<(HTMLButtonElement | null)[]>([])

  const handleIsActive = (index: number) => {
    buttonRefs.current.forEach((button, i) => {
      if (button) {
        button.setAttribute('color', i === index ? 'black' : 'initial')
      }
    })
  }

  const handleClick = (index: number) => {
    setPage(index)
    handleIsActive(index)
  }

  return (
    <Container>
      <NavLeft>
        <div className='logo'>
          <Link to='/'>
            <img src={Logo} alt='' />
          </Link>
        </div>

        <div className='center'>
          <NavButton
            type='button'
            ref={(el) => (buttonRefs.current[0] = el)}
            isActive={page === 0}
            onClick={() => handleClick(0)}
          >
            <img src={Notes} />
          </NavButton>
          <NavButton
            type='button'
            ref={(el) => (buttonRefs.current[1] = el)}
            isActive={page === 1}
            onClick={() => handleClick(1)}
          >
            <img src={Book} alt='' />
          </NavButton>
          <NavButton
            type='button'
            ref={(el) => (buttonRefs.current[2] = el)}
            isActive={page === 2}
            onClick={() => handleClick(2)}
          >
            <img src={Shared} alt='' />
          </NavButton>
        </div>

        <div className='user'>
          <Link to='/'>
            <img src={User} alt='' />
          </Link>
        </div>
      </NavLeft>

      {page === 0 && <NotesPage />}
      {page === 1 && <p>eu sou eu</p>}
      {page === 2 && <p>eu sou eu</p>}
    </Container>
  )
}

export default NavBar
